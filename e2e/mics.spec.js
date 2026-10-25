// Microphone management (dictation/mics.rs, 2026-10-25): an order of
// microphones — the first connected one is used, then Windows' default —
// "<mic> connected · Use it for dictation?" when a new one is plugged in, and
// a microphone unplugged mid-dictation: the dictation ends, what it heard is
// transcribed and kept, and the next one uses the next microphone.
//
// No real device is touched: the test run swaps Windows' list for made-up
// microphones (`YAP_E2E_MICS`, `e2e_mics`), plugs and unplugs them
// (`e2e_mic_plug`, `e2e_mic_unplug`: told as Windows' notifications are), and
// the dictation's mic "opens" one as a quiet tone (dictation/mic.rs), which stops
// when it's unplugged, as a real stream does. The stub engine transcribes a
// dictation as "[STT stub: received …s of audio".
import { test, expect, openSettings, settingsDialog, closeSettings, expectStore, setConfig, pressHotkey, openView, inAnotherApp } from './support/fixtures.js';

const LAPTOP = { id: 'laptop', name: 'Microphone Array (Northwind Audio)' };
const USB = { id: 'usb', name: 'Microphone (Fabrikam USB Mic)' };
const HEADSET = { id: 'headset', name: 'Headset Microphone (Contoso H2)' };
const WEBCAM = { id: 'webcam', name: 'Microphone (Contoso Webcam)' };
const id = (m) => `e2e-${m.id}`;

test.use({
  yapOptions: {
    name: 'mics',
    // The laptop's own mic is Windows' default; the USB mic was the one
    // picked before the order existed (`inputDevice`, migrated).
    env: { YAP_MIC_WARM_MS: '1500', YAP_E2E_MICS: JSON.stringify([LAPTOP, USB]) },
    config: { inputDevice: USB.name },
  },
});

const micStatus = (yap) => yap.invoke('e2e_mic_status');
const recording = async (yap) => (await yap.invoke('e2e_mic_status')).open;
const toast = (main, text) => main.getByRole('status').filter({ hasText: text });
const micRow = (main) => settingsDialog(main).locator('[data-setting="microphone"]');

/** Made-up microphones `devices`, the order `order`, nothing answered "Not now". */
async function setUp(yap, main, devices, order) {
  await yap.invoke('e2e_mics', { devices });
  await setConfig(yap, main, { micOrder: order.map((m) => ({ id: id(m), name: m.name })), micNotNow: [] });
  // Nothing open from the last test.
  await expect.poll(async () => (await micStatus(yap)).open, { timeout: 5_000 }).toBe(false);
}

/** Start a dictation with the hotkey and wait until the made-up mic is live. */
async function startDictating(yap, main) {
  await pressHotkey(main);
  await expect.poll(async () => (await micStatus(yap)).live, { message: 'the mic to be live', timeout: 5_000 }).toBe(true);
}

test('the microphone picked before becomes the first in the order', async ({ yap, main, shot }) => {
  // config.json moved the one pick into the order (config.rs migrate_once v4),
  // and kept it where an older Yap on the same data reads it.
  await expectStore(yap, 'config.json', (c) => c.micOrder?.[0]?.name === USB.name && c.inputDevice === USB.name);
  await openSettings(main, 'Dictation');
  const row = micRow(main);
  await expect(row).toContainText("Yap uses the first one that's connected");
  const list = row.getByRole('list', { name: 'Microphones in order' });
  const items = list.getByRole('listitem');
  await expect(items).toHaveCount(2);
  await expect(items.nth(0)).toContainText('Fabrikam USB Mic');
  await expect(items.nth(0)).toContainText('In use');
  await expect(items.nth(1)).toContainText("Windows' default · Northwind Audio");
  await row.scrollIntoViewIfNeeded();
  await shot(main, '01-order-migrated');
  await closeSettings(main);
});

test('the order: add, move and remove; the next dictation follows it', async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  await openSettings(main, 'Dictation');
  const row = micRow(main);
  const items = row.getByRole('list', { name: 'Microphones in order' }).getByRole('listitem');
  // Add the laptop's mic: second, then moved up.
  await row.getByRole('combobox', { name: 'Add a microphone' }).selectOption(id(LAPTOP));
  await expect(items).toHaveCount(3);
  await expect(items.nth(1)).toContainText('Northwind Audio');
  await row.getByRole('button', { name: 'Move Northwind Audio up' }).click();
  await expect(items.nth(0)).toContainText('Northwind Audio');
  await expect(items.nth(0)).toContainText('In use');
  await expect(items.nth(1)).toContainText('Connected');
  await expectStore(yap, 'config.json', (c) => c.micOrder?.map((m) => m.id).join() === [id(LAPTOP), id(USB)].join());
  await shot(main, '02-order-edited');

  // A dictation now opens the laptop's mic.
  const done = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(done?.raw).toContain('STT stub');
  expect((await micStatus(yap)).last.device).toBe(LAPTOP.name);

  // Removed: the USB mic first again (Settings' order, live: the warm stream
  // on the laptop's mic closes).
  await row.getByRole('button', { name: 'Remove Northwind Audio' }).click();
  await expect(items).toHaveCount(2);
  await expectStore(yap, 'config.json', (c) => c.micOrder?.length === 1 && c.micOrder[0].id === id(USB));
  await expect.poll(async () => (await micStatus(yap)).open, { timeout: 5_000 }).toBe(false);
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect((await micStatus(yap)).last.device).toBe(USB.name);

  // Unplugged, the USB mic says so in its row; Windows' default is in use.
  await yap.invoke('e2e_mic_unplug', { id: USB.id });
  await expect(items.nth(0)).toContainText('Not connected');
  await expect(items.nth(1)).toContainText('In use');
  await shot(main, '03-order-unplugged');
  await closeSettings(main);
});

test('a Bluetooth headset first in the order: used, and never kept warm', async ({ yap, main }) => {
  // The 22 Oct rules hold for whichever microphone in the order it is: the
  // headset's input closes as soon as the dictation is over.
  const BUDS = { id: 'buds', name: 'Headset (Contoso Buds Hands-Free AG Audio)', bluetooth: true };
  await setUp(yap, main, [LAPTOP, USB, BUDS], [BUDS, USB]);
  const done = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(done?.raw).toContain('STT stub');
  const s = await micStatus(yap);
  expect(s.last).toMatchObject({ device: BUDS.name, bluetooth: true });
  // Closed at once, not after the warm window (1.5 s here).
  await expect.poll(async () => (await micStatus(yap)).open, { timeout: 1_000 }).toBe(false);
});

test('a new microphone plugged in: "Use it" puts it first', async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  await yap.invoke('e2e_mic_plug', { device: HEADSET });
  const offer = toast(main, 'Contoso H2 connected');
  await expect(offer).toBeVisible({ timeout: 10_000 });
  await expect(offer).toContainText('Use it for dictation?');
  await expect(offer.getByRole('button', { name: 'Not now' })).toBeVisible();
  await shot(main, '04-offer-in-window');
  await offer.getByRole('button', { name: 'Use it' }).click();
  await expectStore(yap, 'config.json', (c) => c.micOrder?.map((m) => m.id).join() === [id(HEADSET), id(USB)].join());
  // The next dictation uses it.
  await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect((await micStatus(yap)).last.device).toBe(HEADSET.name);

  // Unplugged and plugged in again: it's first in the order, so it's simply
  // used — and said ("Using …"), once.
  await yap.invoke('e2e_mic_unplug', { id: HEADSET.id });
  await yap.invoke('e2e_mic_plug', { device: HEADSET });
  const using = toast(main, 'Using Contoso H2 for dictation');
  await expect(using).toBeVisible({ timeout: 10_000 });
  await shot(main, '05-using-higher-mic');
  // No question for a microphone that's in the order.
  await expect(toast(main, 'Use it for dictation?')).toHaveCount(0);
});

test('"Not now" on the Yap bar, and that microphone is never asked about again', async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  await inAnotherApp(yap);
  await yap.invoke('e2e_mic_plug', { device: WEBCAM });
  const card = yap.overlay.getByRole('status').filter({ hasText: 'Contoso Webcam connected' });
  await expect(card).toBeVisible({ timeout: 10_000 });
  await expect(card).toContainText('Use it for dictation?');
  await expect(card.getByRole('button', { name: 'Use it' })).toBeVisible();
  await expect(card.getByRole('button', { name: 'Not now' })).toBeVisible();
  await yap.overlay.waitForTimeout(300); // the card's entrance
  await shot(yap.overlay, '06-offer-on-bar');
  await yap.invoke('bar_card_action', { id: 'micOffer', action: 'later' });
  await expectStore(yap, 'config.json', (c) => c.micNotNow?.some((m) => m.id === id(WEBCAM)));
  await expect(card).toHaveCount(0);
  expect(yap.readConfig().micOrder.map((m) => m.id)).toEqual([id(USB)]);

  // Plugged in again (the rate limit forgotten): no question.
  await yap.invoke('e2e_mics', { devices: [LAPTOP, USB] });
  await yap.invoke('e2e_mic_plug', { device: WEBCAM });
  await yap.overlay.waitForTimeout(2_500);
  await expect(yap.overlay.getByRole('status').filter({ hasText: 'Contoso Webcam connected' })).toHaveCount(0);
});

test('never during a dictation: the question waits for it to end', async ({ yap, main }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  await startDictating(yap, main);
  await yap.invoke('e2e_mic_plug', { device: HEADSET });
  await main.waitForTimeout(2_500);
  await expect(toast(main, 'Contoso H2 connected')).toHaveCount(0);
  // The dictation goes on with its own mic.
  expect((await micStatus(yap)).device).toBe(USB.name);
  await pressHotkey(main);
  await expect(toast(main, 'Contoso H2 connected')).toBeVisible({ timeout: 10_000 });
});

test('unplugged mid-dictation: it stops, transcribes what it heard, and the next one uses the next mic', async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  await openView(main, 'Home');
  const before = (yap.readJson('history.json') || []).length;
  await startDictating(yap, main);
  await main.waitForTimeout(1_500); // a second and a half said
  await yap.invoke('e2e_mic_unplug', { id: USB.id });

  // Said where the person is looking — once, by what became of the words
  // (pasted into the window it was for) — with the next mic named.
  const alert = toast(main, 'Fabrikam USB Mic disconnected');
  await expect(alert).toBeVisible({ timeout: 10_000 });
  await expect(alert).toContainText('Yap stopped the dictation and pasted what it heard. The next one uses Northwind Audio.');
  // Transcribed and kept: in history, with its audio for Run again.
  await expectStore(yap, 'history.json', (h) => (h || []).length === before + 1);
  const kept = yap.readJson('history.json').reduce((a, b) => (b.ts >= a.ts ? b : a));
  expect(kept.raw).toMatch(/STT stub: received/);
  expect(kept.status, 'pasted into its window').toBeUndefined();
  expect(kept.audio, 'its audio kept a day (Run again)').toBeTruthy();
  await expect(main.getByText(/\[STT stub: received [\d.]+s of audio/).first()).toBeVisible();
  await shot(main, '07-unplugged-mid-dictation');
  const status = await micStatus(yap);
  expect(status.losses).toBeGreaterThanOrEqual(1);
  expect(status.open).toBe(false);

  // The next dictation: the laptop's mic.
  const next = await yap.invoke('e2e_dictate_audio', { seconds: 1 });
  expect(next?.raw).toContain('STT stub');
  expect((await micStatus(yap)).last.device).toBe(LAPTOP.name);
});

test("unplugged mid-dictation with its window gone: on the clipboard, listed as not pasted", async ({ yap, main }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  // The dictation goes to a stand-in window no window has: gone by the paste.
  await yap.invoke('e2e_target_window', { on: true });
  try {
    await openView(main, 'Home');
    await startDictating(yap, main);
    await main.waitForTimeout(1_200);
    await yap.invoke('e2e_mic_unplug', { id: USB.id });
    // One alert, saying where the words are.
    const alert = toast(main, 'Fabrikam USB Mic disconnected');
    await expect(alert).toBeVisible({ timeout: 10_000 });
    await expect(alert).toContainText('What it heard is on the clipboard: press Ctrl+V.');
    await expect(toast(main, 'Your text is copied: press Ctrl+V')).toHaveCount(0);
    await expectStore(yap, 'history.json', (h) => (h || []).some((e) => e.status === 'notPasted' && /microphone was disconnected/.test(e.error || '')));
    await expect(main.getByText('Your microphone was disconnected and its window was gone').first()).toBeVisible();
  } finally {
    await yap.invoke('e2e_target_window', { on: false });
  }
});

test('unplugged before it heard anything: the dictation just ends, saying why', async ({ yap, main, shot }) => {
  await setUp(yap, main, [LAPTOP, USB], [USB]);
  const before = (yap.readJson('history.json') || []).length;
  // The made-up mic's tone arrives as digital silence: nothing heard.
  await yap.invoke('e2e_dictation_mic', { silent: true });
  try {
    await pressHotkey(main);
    await expect.poll(async () => (await micStatus(yap)).open, { timeout: 5_000 }).toBe(true);
    await yap.invoke('e2e_mic_unplug', { id: USB.id });
    await expect(yap.overlay.getByText('Fabrikam USB Mic was disconnected')).toBeVisible({ timeout: 10_000 });
    await shot(yap.overlay, '08-overlay-unplugged-empty');
    await expect(toast(main, 'Fabrikam USB Mic disconnected')).toContainText('before it heard anything');
    await main.waitForTimeout(1_000);
    expect((yap.readJson('history.json') || []).length).toBe(before);
  } finally {
    await yap.invoke('e2e_dictation_mic', { silent: false });
  }
});

test('no microphone left: "No microphone found"', async ({ yap, main }) => {
  await setUp(yap, main, [USB], [USB]);
  await yap.invoke('e2e_mic_unplug', { id: USB.id });
  await pressHotkey(main);
  await expect(yap.overlay.getByText('No microphone found')).toBeVisible({ timeout: 10_000 });
  expect(await recording(yap)).toBe(false);
});
