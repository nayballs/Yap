// Read aloud, anywhere (src-tauri/src/read_aloud.rs, tts.rs's tracked
// read, tts/marks.rs; the reading card src/lib/bar/ReadingCard.svelte):
// text read aloud in the chosen voice with a card on the Yap bar — the
// sentence being read, its current word lit up as it's said (karaoke),
// ⏮ ⏯ ⏭, the speed, Stop, Esc.
//
// Test mode plays nothing: the voice is the stand-in (silence as long as
// the words would take, ~250 ms a word, on a simulated clock), what would
// have been said goes to the debug-only `tts_e2e_spoken`, and the words'
// times are worked out from that silent audio's length (the "estimate"
// timing). Test mode never reads another app's selection (the robot suite,
// e2e-real/read-aloud.real.js, does that in windows it opens), so here the
// text comes in through `read_aloud_text` — the command a note's or an
// answer's Read aloud, and the in-page shortcut, use.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, openSettings, closeSettings, settingsDialog, expectStore } from './support/fixtures.js';
import { SCREENSHOTS } from './support/yap.js';

const PARAGRAPH = [
  'Reading aloud helps anyone who finds long pages hard going.',
  'Yap reads the text you select, one sentence at a time, and lights up each word as it says it.',
  'You can pause, skip a sentence, or change the speed.',
  'Press Esc when you have heard enough.',
].join(' ');

const state = (yap) => yap.invoke('read_aloud_state');
const spoken = (yap) => yap.invoke('tts_e2e_spoken');
const speaking = async (yap) => (await yap.invoke('tts_state')).speaking;
const card = (yap) => yap.overlay.locator('[data-region="reading"].card');

async function setConfig(yap, main, patch) {
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
  // Settings' copy auto-saves what it adopted (400 ms later): settled.
  await main.waitForTimeout(700);
  await expectStore(yap, 'config.json', (c) => Object.entries(patch).every(([k, v]) => JSON.stringify(c[k]) === JSON.stringify(v)));
}

/** The reading card alone, as a picture (for the report and the README). */
async function cardShot(yap, name) {
  const file = path.join(SCREENSHOTS, 'read-aloud', `${name}.png`);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  await card(yap).screenshot({ path: file, animations: 'disabled' });
}

test.beforeEach(async ({ yap }) => {
  await yap.invoke('tts_e2e_spoken', { clear: true });
});

test.afterEach(async ({ yap }) => {
  await yap.invoke('read_aloud_action', { action: 'stop' }).catch(() => {});
  await yap.invoke('tts_stop').catch(() => {});
});

test('selected text is read sentence by sentence, each word lit up as it is said', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { ttsRate: 1, barTheme: 'light', readingFont: 'default', readingSize: 'default' });
  const id = await yap.invoke('read_aloud_text', { text: PARAGRAPH, markdown: false, source: 'yap' });
  expect(id).toMatch(/^read:\d+$/);

  // The card on the bar: the first sentence, "1 of 4".
  const c = card(yap);
  await expect(c).toBeVisible();
  await expect(c).toHaveAttribute('data-phase', 'reading');
  await expect(c.locator('.text')).toHaveText('Reading aloud helps anyone who finds long pages hard going.');
  await expect(c).toContainText('1 of 4');
  // What the voice was given: the whole text, sentence by sentence.
  const [said] = await spoken(yap);
  expect(said.messageId).toBe(id);
  expect(said.sentences).toEqual([
    'Reading aloud helps anyone who finds long pages hard going.',
    'Yap reads the text you select, one sentence at a time, and lights up each word as it says it.',
    'You can pause, skip a sentence, or change the speed.',
    'Press Esc when you have heard enough.',
  ]);

  // Karaoke: the lit word moves on through the sentence.
  const lit = () => c.locator('.w.now').textContent().catch(() => null);
  await expect.poll(lit, { timeout: 5_000 }).toBe('aloud');
  await expect.poll(async () => (await state(yap)).word, { timeout: 5_000 }).toBeGreaterThanOrEqual(3);
  const s = await state(yap);
  expect(s.timing).toBe('estimate');
  expect(s.engine).toBe('windows');
  await cardShot(yap, '01-reading-light');
  await shot(yap.overlay, '01-bar-reading');

  // The sentence after it, then the next ones.
  await expect.poll(async () => (await state(yap)).index, { timeout: 10_000 }).toBe(1);
  await expect(c).toContainText('2 of 4');
  await expect(c.locator('.text')).toContainText('lights up each word');

  // ⏸ holds it: the same word, sentence, "Paused"…
  await c.getByRole('button', { name: 'Pause' }).click();
  await expect(c).toHaveAttribute('data-phase', 'paused');
  await expect(c.locator('.chip')).toContainText('Paused');
  const held = await state(yap);
  await main.waitForTimeout(700);
  const still = await state(yap);
  expect([still.index, still.word]).toEqual([held.index, held.word]);
  expect(await speaking(yap)).toBe(true);
  await cardShot(yap, '02-paused');
  // …▶ goes on from there.
  await c.getByRole('button', { name: 'Resume' }).click();
  await expect(c).toHaveAttribute('data-phase', 'reading');
  await expect.poll(async () => (await state(yap)).word ?? -1, { timeout: 5_000 }).not.toBe(held.word ?? -1);

  // ⏭ the next sentence, ⏮ back again.
  const before = (await state(yap)).index;
  await c.getByRole('button', { name: 'Next sentence' }).click();
  await expect.poll(async () => (await state(yap)).index).toBe(before + 1);
  await c.getByRole('button', { name: 'Back a sentence' }).click();
  await expect.poll(async () => (await state(yap)).index).toBeLessThanOrEqual(before + 1);

  // Faster: 1.25×, kept as the voice's speed (Settings' copy follows).
  await c.getByRole('button', { name: 'Faster' }).click();
  await expect(c.locator('.rate')).toHaveText('1.25×');
  await expectStore(yap, 'config.json', (cfg) => cfg.ttsRate === 1.25);
  expect((await spoken(yap)).at(-1).rate).toBe(1.25);
  await c.getByRole('button', { name: 'Slower' }).click();
  await expect(c.locator('.rate')).toHaveText('1×');
  await expectStore(yap, 'config.json', (cfg) => cfg.ttsRate === 1);

  // Stop: the voice stops and the card goes.
  await c.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(c).toHaveCount(0);
  await expect.poll(() => speaking(yap)).toBe(false);
  expect((await state(yap)).active).toBe(false);
});

test('Esc stops it (the bar watches Esc while the card is up), and so does the end of the text', async ({ yap }) => {
  await yap.invoke('read_aloud_text', { text: PARAGRAPH, markdown: false, source: 'yap' });
  await expect(card(yap)).toBeVisible();
  await yap.invoke('bar_simulate', { escape: true });
  await expect(card(yap)).toHaveCount(0);
  await expect.poll(() => speaking(yap)).toBe(false);

  // Read to the end: the card goes by itself.
  await yap.invoke('read_aloud_text', { text: 'Short and sweet.', markdown: false, source: 'yap' });
  await expect(card(yap)).toBeVisible();
  await expect(card(yap)).toHaveCount(0, { timeout: 5_000 });
});

test('the reading card in Dark, in OpenDyslexic at the largest size', async ({ yap, main }) => {
  await setConfig(yap, main, { barTheme: 'dark', readingFont: 'dyslexic', readingSize: 'larger' });
  await yap.invoke('read_aloud_text', { text: PARAGRAPH, markdown: false, source: 'yap' });
  const c = card(yap);
  await expect(c).toBeVisible();
  await expect.poll(() => c.locator('.w.now').count(), { timeout: 5_000 }).toBe(1);
  // The reading font and size from Settings → Appearance; roomy lines.
  const look = await c.locator('.text').evaluate((el) => {
    const s = getComputedStyle(el);
    return { font: s.fontFamily, size: parseFloat(s.fontSize), line: parseFloat(s.lineHeight) / parseFloat(s.fontSize) };
  });
  expect(look.font).toContain('OpenDyslexic');
  expect(look.size).toBeGreaterThan(18);
  expect(look.line).toBeGreaterThan(1.6);
  // The bar's dark surface.
  expect(await c.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgb(28, 26, 22)');
  await expect.poll(async () => (await state(yap)).word ?? 0, { timeout: 5_000 }).toBeGreaterThanOrEqual(4);
  await cardShot(yap, '03-dark-opendyslexic');
  await yap.invoke('read_aloud_action', { action: 'pause' });
  await setConfig(yap, main, { barTheme: 'light', readingFont: 'dyslexic', readingSize: 'default' });
  await expect.poll(() => c.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe('rgb(255, 255, 255)');
  await cardShot(yap, '04-light-opendyslexic');
  await setConfig(yap, main, { readingFont: 'default' });
});

test('a long text is made a sentence ahead of the voice, never all at once', async ({ yap, main }) => {
  await setConfig(yap, main, { ttsRate: 2 });
  const long = Array.from({ length: 1_000 }, (_, i) => `This is sentence ${i + 1} of a long text.`).join(' ');
  expect(long.length).toBeGreaterThan(30_000);
  await yap.invoke('read_aloud_text', { text: long, markdown: false, source: 'yap' });
  await expect(card(yap)).toContainText('1 of 1000');
  for (let i = 0; i < 6; i++) {
    const s = await state(yap);
    // The one being read and the next, no more (the card's index can lag
    // the voice by a tick: the speaker's own rule is unit-tested exactly).
    expect(s.made).toBeLessThanOrEqual(s.index + 3);
    await yap.overlay.waitForTimeout(250);
  }
  await expect.poll(async () => (await state(yap)).index, { timeout: 8_000 }).toBeGreaterThanOrEqual(2);
  const s = await state(yap);
  expect(s.made).toBeLessThanOrEqual(s.index + 3);
  expect(s.made).toBeLessThan(30);
  expect(s.count).toBe(1000);
  await setConfig(yap, main, { ttsRate: 1 });
});

test('nothing selected: the card says how, naming the shortcut, and 🔊 on the bar does the same', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { readHotkey: 'kb:ctrl+alt+82' });
  // Test mode reads no other app: as if nothing were selected.
  await yap.invoke('read_aloud_selection');
  const c = card(yap);
  await expect(c).toHaveAttribute('data-phase', 'note');
  await expect(c).toContainText('Nothing selected');
  await expect(c).toContainText('press Ctrl + Alt + R or click 🔊 on the Yap bar');
  await cardShot(yap, '05-nothing-selected');
  await c.getByRole('button', { name: 'Close' }).click();
  await expect(c).toHaveCount(0);

  // 🔊 in the pill opened on hover, with its tooltip.
  await yap.invoke('bar_simulate', { pointer: 'pill' });
  const read = yap.overlay.getByRole('button', { name: 'Read aloud' });
  await expect(read).toBeVisible();
  await read.hover();
  await expect(yap.overlay.getByRole('tooltip')).toHaveText(/Read aloud\s*Ctrl \+ Alt \+ R/);
  await shot(yap.overlay, '06-pill-read-aloud');
  await read.click();
  await expect(c).toContainText('Nothing selected');
  await yap.invoke('bar_simulate', { pointer: 'away' });
  await setConfig(yap, main, { readHotkey: '' });
});

test('a note has Read aloud (its button and its menu), and it reads on in the card', async ({ yap, main, shot }) => {
  await yap.invoke('note_create', { title: 'Reading plan', content: 'The launch is on **Friday**.\n\n- Book the room\n- Send the invites' });
  await openView(main, 'Notes');
  await main.locator('.items .item').filter({ hasText: 'Reading plan' }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Reading plan');
  const button = main.getByRole('button', { name: 'Read aloud', exact: true });
  await button.click();
  await expect(main.getByRole('button', { name: 'Stop reading', exact: true })).toBeVisible();
  const [said] = await spoken(yap);
  expect(said.messageId).toMatch(/^note:\d+$/);
  expect(said.sentences).toEqual(['Reading plan.', 'The launch is on Friday.', 'Book the room.', 'Send the invites.']);
  await expect(card(yap).locator('.text')).toHaveText('Reading plan.');
  await shot(main, '07-note-reading');
  // Stop from the note.
  await main.getByRole('button', { name: 'Stop reading', exact: true }).click();
  await expect(card(yap)).toHaveCount(0);

  // The note's right-click menu says Read aloud too.
  await main.locator('.items .item').filter({ hasText: 'Reading plan' }).click({ button: 'right' });
  const menu = main.getByRole('menu');
  await menu.getByRole('menuitem', { name: 'Read aloud' }).click();
  await expect(card(yap)).toBeVisible();
  await main.keyboard.press('Escape');
});

test('a Chat answer read aloud shows the same card, and the shortcut in Yap reads the page selection', async ({ yap, main }) => {
  await setConfig(yap, main, { readHotkey: 'kb:ctrl+alt+82' });
  // An answer's ▶ (speech.svelte.js): its id, so ▶ shows ■.
  await yap.invoke('read_aloud_text', { text: 'The answer is **Friday**. Sam books the room.', messageId: 'chat:999:1', markdown: true, source: 'chat' });
  await expect(card(yap)).toBeVisible();
  expect((await state(yap)).messageId).toBe('chat:999:1');
  expect((await yap.invoke('tts_state')).messageId).toBe('chat:999:1');
  await yap.invoke('tts_stop');
  await expect(card(yap)).toHaveCount(0);

  // The shortcut pressed in Yap's own window (the global hook never sees it
  // there): what's selected on the page.
  await yap.invoke('note_create', { title: 'Shortcut note', content: 'Select this sentence to hear it.' });
  await openView(main, 'Notes');
  await main.locator('.items .item').filter({ hasText: 'Shortcut note' }).click();
  const editor = main.locator('textarea.raw');
  await editor.focus();
  await editor.evaluate((el) => el.setSelectionRange(0, el.value.length));
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await main.keyboard.press('Control+Alt+KeyR');
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text)).toEqual(['Select this sentence to hear it.']);
  await expect(card(yap)).toBeVisible();
  // Again with nothing new selected: it stops.
  await editor.evaluate((el) => el.setSelectionRange(0, 0));
  await main.keyboard.press('Control+Alt+KeyR');
  await expect(card(yap)).toHaveCount(0);
  await setConfig(yap, main, { readHotkey: '' });
});

test('Settings: the Read aloud shortcut on Shortcuts and on Voice Assistant, never clashing', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const row = dialog.locator('[data-setting="shortcut-read"]');
  await expect(row).toContainText('Read aloud');
  await expect(row).toContainText('Select text in any app');
  await shot(main, '08-settings-shortcuts');
  await openSettings(main, 'Voice Assistant');
  const group = settingsDialog(main).getByRole('group', { name: 'Read aloud' });
  await group.scrollIntoViewIfNeeded();
  await expect(group).toContainText('Read aloud shortcut');
  await shot(main, '09-settings-voice-read-aloud');
  // Search finds it.
  await closeSettings(main);
  await openSettings(main);
  await settingsDialog(main).getByRole('searchbox').fill('read aloud');
  await expect(settingsDialog(main).getByText('Read aloud shortcut').first()).toBeVisible();
  await closeSettings(main);
  // A clash with the dictation hotkey (F24) is refused by Rust too: left
  // unbound until it's free.
  await setConfig(yap, main, { readHotkey: 'kb:135' });
  await expectStore(yap, 'config.json', (c) => c.readHotkey === 'kb:135');
  await setConfig(yap, main, { readHotkey: '' });
});
