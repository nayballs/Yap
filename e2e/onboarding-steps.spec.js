// The first-run setup's newer steps (2026-10-29; shell/guide.rs,
// dictation/mic_quiz.rs, onboarding/*Step.svelte), each shown, skipped and
// done:
// - Languages: "I only speak English" saves English and moves on in one
//   click; a language picked is saved as it's picked, and the speech model's
//   cover of them is said.
// - The microphone: the mic quiz listens on every made-up microphone at once
//   (`e2e_mics`, each sounding as `e2e_mic_quiz` says), marks the best, gives
//   a Bluetooth headset its note, and "Use this one" puts it first.
// - The calendar: Skip for now never shows it again; a calendar connected
//   (a local iCal feed) ticks it and it's never shown again either.
// - The typing test: typed, then said with the dictation shortcut (the stub
//   transcribes), both speeds shown and kept as numbers only.
// Made-up microphones and calendars only.
import { test as base, expect, expectStore, pressHotkey } from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';

const test = base.extend({
  yapOptions: [{ name: 'onboarding-steps', config: { myLanguages: [] } }, { scope: 'worker' }],
});

const MICS = [
  { id: 'usb', name: 'Microphone (Fabrikam USB Mic)', default: true },
  { id: 'buds', name: 'Headset (Contoso Buds Hands-Free AG Audio)', bluetooth: true },
  { id: 'array', name: 'Microphone Array (Northwind Audio)' },
];

/** The setup, opened afresh on its first step (what was answered forgotten). */
async function openSetup(yap) {
  await yap.invoke('e2e_guide_reset');
  await yap.invoke('open_onboarding');
  const ob = yap.onboarding;
  await expect(ob.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  return ob;
}

const stepIds = (ob) => ob.locator('nav.dots .dot').evaluateAll((els) => els.map((e) => e.dataset.step));
const continueBtn = (ob) => ob.getByRole('button', { name: 'Continue →' });

// A made-up microphone (its tone), never the PC's own: the mic step's
// check and the typing test's dictation open it.
test.beforeEach(async ({ yap }) => {
  await yap.invoke('e2e_mics', { devices: [MICS[0]] });
});

test.afterEach(async ({ yap }) => {
  await yap.invoke('close_onboarding').catch(() => {});
  // The mic check let go first, so nothing reaches for the PC's own mic.
  await yap.invoke('set_mic_test', { on: false }).catch(() => {});
  await yap.onboarding.waitForTimeout(300);
  await yap.invoke('e2e_mics', { devices: null }).catch(() => {});
});

test('the languages step: English only in one click, or the languages picked, with what the speech model makes of them', async ({ yap, shot }) => {
  const ob = await openSetup(yap);
  expect(await stepIds(ob)).toEqual(['model', 'languages', 'mic', 'cleanup', 'calendar', 'tray', 'try', 'typing']);
  await ob.getByRole('button', { name: "I'll choose later" }).click();
  await expect(ob.getByRole('heading', { level: 1, name: 'Which languages do you speak?' })).toBeVisible();
  await shot(ob, '01-languages');

  // One click for someone who speaks only English: saved, and on to the mic.
  await ob.getByRole('button', { name: /I only speak English/ }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.myLanguages) === '["en"]');
  await expect(ob.getByRole('heading', { level: 1, name: "Let's check your microphone" })).toBeVisible();

  // Back: French added beside English, saved as it's picked.
  await ob.getByRole('button', { name: '← Back' }).click();
  const chips = ob.getByRole('group', { name: 'Languages you speak' });
  await expect(chips.locator('[data-language="en"]')).toHaveAttribute('aria-pressed', 'true');
  await chips.locator('[data-language="fr"]').click();
  await expect(chips.locator('[data-language="fr"]')).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'config.json', (c) => c.myLanguages?.includes('fr') && c.myLanguages?.includes('en'));
  // A language found by name.
  await ob.getByRole('searchbox', { name: 'Find another language' }).fill('Welsh');
  await ob.getByRole('list', { name: 'Languages found' }).getByRole('button', { name: /Welsh/ }).click();
  await expectStore(yap, 'config.json', (c) => c.myLanguages?.includes('cy'));
  await shot(ob, '02-languages-picked');
  // Taken off again.
  await chips.locator('[data-language="cy"]').click();
  await chips.locator('[data-language="fr"]').click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.myLanguages) === '["en"]');
  // With a speech model in use, its cover of them is said.
  if ((await yap.invoke('get_config')).modelSize && (await yap.invoke('installed_models')).length) {
    await expect(ob.getByTestId('language-coverage')).toBeVisible();
  }
});

test('the mic step: the quiz listens on every microphone at once, marks the best, and Use this one puts it first', async ({ yap, shot }) => {
  await yap.invoke('e2e_mics', { devices: MICS });
  // The USB mic clear, the headset in its call profile a little quieter, the
  // laptop's array far away in a noisy room.
  await yap.invoke('e2e_mic_quiz', { profiles: { usb: [0.25, 0.001], buds: [0.2, 0.002], array: [0.05, 0.06] } });
  try {
    const ob = await openSetup(yap);
    await ob.getByRole('button', { name: "I'll choose later" }).click();
    await continueBtn(ob).click();
    await expect(ob.getByRole('heading', { level: 1, name: "Let's check your microphone" })).toBeVisible();
    const quiz = ob.getByTestId('mic-quiz');
    await expect(quiz).toHaveAttribute('data-phase', 'idle');
    await quiz.getByRole('button', { name: 'Start the mic quiz' }).click();
    await expect(quiz).toHaveAttribute('data-phase', 'listening');
    await expect(quiz.locator('.mic')).toHaveCount(3);
    await shot(ob, '03-mic-quiz-listening');
    await expect(quiz).toHaveAttribute('data-phase', 'done', { timeout: 15_000 });
    await expect(quiz.getByRole('status')).toContainText('Your best microphone is Fabrikam USB Mic');
    const usb = quiz.locator('[data-mic="e2e-usb"]');
    await expect(usb.locator('.badge.best')).toHaveText('Best');
    await expect(usb).toHaveAttribute('data-quality', 'clear');
    // The headset's honest note; the array's noise.
    await expect(quiz.locator('[data-mic="e2e-buds"] .note')).toContainText('Bluetooth headsets sound worse while their mic is on');
    await expect(quiz.locator('[data-mic="e2e-array"]')).toHaveAttribute('data-quality', 'noisy');
    await shot(ob, '04-mic-quiz-done');
    // The array picked: first in the order.
    await quiz.locator('[data-mic="e2e-array"]').getByRole('button', { name: 'Use this one' }).click();
    await expectStore(yap, 'config.json', (c) => c.micOrder?.[0]?.id === 'e2e-array');
    await expect(quiz.locator('.ok')).toContainText('Northwind Audio is first in your microphones now');
    // The best one.
    await usb.getByRole('button', { name: 'Use this one' }).click();
    await expectStore(yap, 'config.json', (c) => c.micOrder?.[0]?.id === 'e2e-usb');
  } finally {
    const cfg = await yap.invoke('get_config');
    await yap.invoke('save_config', { cfg: { ...cfg, micOrder: [] } });
  }
});

test('the calendar step: skipped, it never shows again', async ({ yap, shot }) => {
  const ob = await openSetup(yap);
  await ob.locator('nav.dots .dot').first().waitFor();
  await ob.getByRole('button', { name: "I'll choose later" }).click();
  await continueBtn(ob).click();
  await continueBtn(ob).click();
  await ob.getByRole('button', { name: 'Maybe later' }).click();
  await expect(ob.getByRole('heading', { level: 1, name: "Meeting notes, with everyone's names" })).toBeVisible();
  await expect(ob.getByRole('button', { name: 'Connect calendar' })).toBeVisible();
  await shot(ob, '05-calendar-step');
  // The main window's own dialog, over the setup; closed again.
  await ob.getByRole('button', { name: 'Connect calendar' }).click();
  const dialog = ob.getByRole('dialog', { name: /calendar/i });
  await expect(dialog.getByRole('button', { name: 'iCloud or another calendar' })).toBeVisible();
  await shot(ob, '06-calendar-dialog');
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(dialog).toBeHidden();
  await ob.getByRole('button', { name: 'Skip for now' }).click();
  await expect(ob.getByRole('heading', { level: 1, name: 'Yap lives in your tray' })).toBeVisible();
  await expectStore(yap, 'onboarding.json', (g) => g.calendarStep === 'skipped');

  // Shown again: no calendar step.
  await yap.invoke('close_onboarding');
  await yap.invoke('open_onboarding');
  await expect(ob.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  await expect.poll(() => stepIds(ob)).toEqual(['model', 'languages', 'mic', 'cleanup', 'tray', 'try', 'typing']);
});

test('the calendar step: a calendar connected ticks it, and it never shows again', async ({ yap, shot }) => {
  const feed = await startCalendarFeed();
  const soon = new Date(Date.now() + 2 * 3600_000);
  feed.set(calendarFile('Work', [vevent({ uid: 'standup', summary: 'Design review', start: soon, end: new Date(soon.getTime() + 1800_000) })]));
  try {
    const ob = await openSetup(yap);
    await ob.getByRole('button', { name: "I'll choose later" }).click();
    await continueBtn(ob).click();
    await continueBtn(ob).click();
    await ob.getByRole('button', { name: 'Maybe later' }).click();
    await ob.getByRole('button', { name: 'Connect calendar' }).click();
    const dialog = ob.getByRole('dialog', { name: /calendar/i });
    await dialog.getByRole('button', { name: 'iCloud or another calendar' }).click();
    await dialog.locator('input').fill(feed.url);
    await dialog.locator('input').press('Enter');
    await expect(ob.getByTestId('calendar-connected')).toContainText('Calendar connected', { timeout: 15_000 });
    await expect(dialog).toBeHidden({ timeout: 10_000 });
    await expectStore(yap, 'onboarding.json', (g) => g.calendarStep === 'connected');
    await shot(ob, '07-calendar-connected');
    // Continue now, no Skip.
    await expect(ob.getByRole('button', { name: 'Skip for now' })).toHaveCount(0);
    await continueBtn(ob).click();
    await expect(ob.getByRole('heading', { level: 1, name: 'Yap lives in your tray' })).toBeVisible();
    // Shown again (even after the record's forgotten): a calendar is connected.
    await yap.invoke('close_onboarding');
    await yap.invoke('e2e_guide_reset');
    await yap.invoke('open_onboarding');
    await expect(ob.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
    await expect.poll(() => stepIds(ob)).not.toContain('calendar');
  } finally {
    const s = await yap.invoke('calendar_status');
    for (const c of s.connections) await yap.invoke('calendar_disconnect', { id: c.id });
    await feed.close();
  }
});

test('the typing test: typed, then said, both speeds shown and only the numbers kept; Finish works either way', async ({ yap, shot }) => {
  const ob = await openSetup(yap);
  // Straight to the last step by the dots' own order.
  await ob.getByRole('button', { name: "I'll choose later" }).click();
  for (let i = 0; i < 2; i++) await continueBtn(ob).click();
  await ob.getByRole('button', { name: 'Maybe later' }).click();
  await ob.getByRole('button', { name: 'Skip for now' }).click();
  await continueBtn(ob).click();
  await expect(ob.getByRole('heading', { level: 1, name: 'Try it' })).toBeVisible();
  await continueBtn(ob).click();
  await expect(ob.getByRole('heading', { level: 1, name: 'How fast do you type?' })).toBeVisible();
  await shot(ob, '08-typing-ready');

  const tt = ob.getByTestId('typing-test');
  const passage = (await tt.locator('blockquote').innerText()).trim();
  const box = tt.getByRole('textbox', { name: 'Type the passage here' });
  await box.click();
  // Typed at a person's pace (about 100 words a minute).
  await box.pressSequentially(passage.slice(0, 60), { delay: 120 });
  await box.press('Enter');
  await expect(tt).toHaveAttribute('data-phase', 'speak');
  const typed = Number(await tt.getByTestId('typing-wpm').innerText());
  expect(typed).toBeGreaterThan(40);
  expect(typed).toBeLessThan(250);
  await expectStore(yap, 'onboarding.json', (g) => g.typingWpm === typed && g.speakingWpm == null);
  await shot(ob, '09-typing-speak');

  // Said with the dictation shortcut (the stub's transcript counts its words).
  await pressHotkey(ob);
  await ob.waitForTimeout(1_500);
  await pressHotkey(ob);
  await expect(tt).toHaveAttribute('data-phase', 'done', { timeout: 15_000 });
  const spoken = Number(await tt.getByTestId('speaking-wpm').innerText());
  expect(spoken).toBeGreaterThan(0);
  await expectStore(yap, 'onboarding.json', (g) => g.typingWpm === typed && g.speakingWpm === spoken);
  // Numbers only: nothing typed or said in the file.
  const file = JSON.stringify(yap.readJson('onboarding.json'));
  expect(file).not.toContain(passage.slice(0, 20));
  await shot(ob, '10-typing-done');

  await ob.getByRole('button', { name: /^Finish/ }).click();
  await expect.poll(() => yap.invoke('plugin:window|is_visible', { label: 'onboarding' })).toBe(false);
});
