// Dictation from the phone (docs/architecture/link.md, "Dictation"): the
// mode switch near Talk — Ask Yap · Dictate to PC · Dictate to phone,
// remembered — the start each sends, and the result under Talk: "Pasted
// into Notepad" with the text, the reason when it couldn't paste, or the
// text copied on this phone with Copy again. The PC is mocked, as in
// link-page.spec.js.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, ask, snapshot, ORIGIN, PC, MODELS } from './support/harness.js';

const TEXT = 'Book the dentist for Thursday morning, and ask about the bill.';

/** Picks what Talk does (a tap on its segment, which is the radio itself). */
const pick = (page, name) => page.getByRole('radio', { name, exact: true }).click();

test.describe('dictation', () => {
  test.beforeEach(async ({ page }) => {
    await servePage(page);
    await seedDevice(page);
  });

  test('the mode switch: Ask by default, remembered, the model chip and Clearer sound for Ask only', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await expect(page.getByTestId('mode-ask')).toBeChecked();
    await expect(page.getByTestId('model-chip')).toBeVisible();
    await expect(page.getByText('Clearer sound in a headset')).toBeVisible();
    await expect(page.getByText('Ask Yap anything: tap Talk, or hold it while you speak.')).toBeVisible();

    await pick(page, 'Dictate to PC');
    await expect(page.getByTestId('mode-pc')).toBeChecked();
    await expect(page.getByTestId('model-chip')).toBeHidden();
    await expect(page.getByText('Clearer sound in a headset')).toBeHidden();
    await expect(page.getByText('Yap types it into the window in front on your PC.')).toBeVisible();
    // Hands-free and its wait are the same for every mode.
    await expect(page.getByText('Hands-free')).toBeVisible();
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.mode')))).toBe('pc');

    // Back later: still dictating to the PC.
    await open(page);
    await ready(page, pc);
    await expect(page.getByTestId('mode-pc')).toBeChecked();
    expect((await snapshot(page)).mode).toBe('pc');

    await pick(page, 'Dictate to phone');
    await expect(page.getByTestId('mode-phone')).toBeChecked();
    await expect(page.getByText('Yap writes it out and copies it on this phone.')).toBeVisible();
    await pick(page, 'Ask Yap');
    await expect(page.getByTestId('model-chip')).toBeVisible();
    await expect(page.getByText('Clearer sound in a headset')).toBeVisible();
  });

  test('Ask still starts a question: no target, its heard in the conversation', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc, 300);
    expect(pc.of('start')[0]).toEqual({ type: 'start', mode: 'ask' });
    pc.send({ type: 'heard', text: 'Is it going to rain?' });
    await expect(page.getByTestId('heard').last()).toHaveText('Is it going to rain?');
    await expect(page.getByTestId('result')).toBeHidden();
  });

  test('Dictate to PC: its start, the frames, stop; "Pasted into Notepad" and the text under Talk', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await pick(page, 'Dictate to PC');
    await ask(page, pc, 600);
    expect(pc.of('start')[0]).toEqual({ type: 'start', mode: 'dictate', target: 'pc' });
    // Every frame between start and stop, as for a question.
    const order = pc.log.map((e) => (e.text ? e.text.type : 'frame')).filter((t) => t !== 'ping');
    expect(order.indexOf('frame')).toBeGreaterThan(order.indexOf('start'));
    expect(order.indexOf('stop')).toBeGreaterThan(order.lastIndexOf('frame'));

    pc.send({ type: 'state', phase: 'transcribing' });
    await expect(page.getByTestId('phase')).toHaveText(`Typing it on ${PC}`);
    // No "Thinking" line: a dictation has no steps.
    await expect(page.getByTestId('step')).toBeHidden();
    pc.send({ type: 'heard', text: TEXT, to: 'pc' });
    pc.send({ type: 'pasted', ok: true, app: 'Notepad' });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('result-line')).toHaveText('Pasted into Notepad');
    await expect(page.getByTestId('result-text')).toHaveText(TEXT);
    await expect(page.getByTestId('copy')).toBeHidden();
    await expect(page.getByTestId('phase')).toHaveText('Ready');
    // Not a question: nothing in the conversation (which is Ask's).
    await expect(page.getByTestId('turn')).toHaveCount(0);
    expect((await snapshot(page)).result).toMatchObject({ to: 'pc', status: 'pasted', app: 'Notepad', text: TEXT });

    // The next one couldn't be pasted: the reason, in the PC's words. The
    // last result went when it started.
    await ask(page, pc, 300);
    await expect(page.getByTestId('result')).toBeHidden();
    expect(pc.of('start')[1]).toEqual({ type: 'start', mode: 'dictate', target: 'pc' });
    pc.send({ type: 'state', phase: 'transcribing' });
    pc.send({ type: 'heard', text: 'Hello there.', to: 'pc' });
    pc.send({ type: 'pasted', ok: false, reason: 'No text box in front: copied on the PC instead' });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('result-line')).toHaveText('No text box in front: copied on the PC instead');
    await expect(page.getByTestId('result-text')).toHaveText('Hello there.');
    await expect(page.getByTestId('result')).toHaveAttribute('data-status', 'failed');
  });

  test('the PC stops a dictation itself: the phone stops listening, and the result still comes', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await pick(page, 'Dictate to PC');
    await page.getByTestId('talk').click();
    await expect.poll(() => pc.of('start').length).toBe(1);
    pc.send({ type: 'state', phase: 'listening' });
    await expect.poll(() => pc.frames.length, { timeout: 5000 }).toBeGreaterThan(10);
    // The shortcut pressed on the PC.
    pc.send({ type: 'state', phase: 'transcribing' });
    await expect.poll(async () => (await snapshot(page)).local).toBe('idle');
    await expect(page.getByTestId('note')).toHaveText('Stopped on your PC.');
    const frames = pc.frames.length;
    await page.waitForTimeout(300);
    expect(pc.frames.length - frames).toBeLessThan(3);
    pc.send({ type: 'heard', text: 'Short one.', to: 'pc' });
    pc.send({ type: 'pasted', ok: true, app: 'Word' });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('result-line')).toHaveText('Pasted into Word');
    expect(pc.of('stop').length).toBe(0);
  });

  test('Dictate to phone: the text comes back with no paste, copied here, and Copy again', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: ORIGIN });
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await pick(page, 'Dictate to phone');
    await ask(page, pc, 400);
    expect(pc.of('start')[0]).toEqual({ type: 'start', mode: 'dictate', target: 'phone' });
    pc.send({ type: 'state', phase: 'transcribing' });
    await expect(page.getByTestId('phase')).toHaveText('Writing it out');
    pc.send({ type: 'heard', text: TEXT, to: 'phone' });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('result-line')).toHaveText('Copied');
    await expect(page.getByTestId('result-text')).toHaveText(TEXT);
    await expect(page.getByTestId('copy')).toHaveText('Copy again');
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(TEXT);
    await expect(page.getByTestId('turn')).toHaveCount(0);

    // Something else copied since: Copy again puts it back.
    await page.evaluate(() => navigator.clipboard.writeText('something else'));
    await page.getByTestId('copy').click();
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(TEXT);
    await expect(page.getByTestId('result-line')).toHaveText('Copied');
  });

  test("Dictate to phone when the browser won't copy by itself: Copy does it", async ({ page }) => {
    await page.addInitScript(() => {
      // A browser that only copies on a tap (no permission given here).
      const real = navigator.clipboard;
      let tapped = false;
      document.addEventListener('pointerdown', () => (tapped = true), true);
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: {
          writeText: async (t) => {
            if (!tapped) throw new DOMException('Not allowed', 'NotAllowedError');
            window.__copied = t;
          },
          readText: () => real.readText(),
        },
      });
    });
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    // Picked and Talk tapped with the keyboard: no pointer, so no copy yet.
    await page.getByTestId('mode-phone').focus();
    await page.keyboard.press('Space');
    await page.getByTestId('talk').focus();
    await page.keyboard.press('Enter');
    await expect.poll(() => pc.of('start').length).toBe(1);
    await expect.poll(() => pc.frames.length, { timeout: 5000 }).toBeGreaterThan(10);
    await page.keyboard.press('Enter');
    await expect.poll(() => pc.of('stop').length).toBe(1);
    pc.send({ type: 'heard', text: 'Milk and eggs.', to: 'phone' });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('result-line')).toHaveText('Ready to copy');
    await expect(page.getByTestId('copy')).toHaveText('Copy');
    await page.getByTestId('copy').click();
    await expect(page.getByTestId('result-line')).toHaveText('Copied');
    expect(await page.evaluate(() => window.__copied)).toBe('Milk and eggs.');
  });

  test('typing into the PC off for this phone: said, and nothing recorded; the PC refusing ends it too', async ({ page }) => {
    const pc = await mockPc(page, {
      onHello: (hello, conn, pc) => pc.sendTo(conn, { type: 'ready', pc: PC, model: MODELS[0], models: MODELS, mayType: false }),
    });
    await open(page);
    await ready(page, pc);
    await pick(page, 'Dictate to PC');
    await expect(page.getByTestId('mode-hint')).toBeVisible();
    await expect(page.getByTestId('mode-hint')).toContainText('May type into this PC');
    await page.getByTestId('talk').click();
    await expect(page.getByTestId('error')).toContainText('Typing into your PC is off for this phone');
    await page.waitForTimeout(300);
    expect(pc.of('start').length).toBe(0);
    // Dictate to phone still works without it.
    await pick(page, 'Dictate to phone');
    await expect(page.getByTestId('mode-hint')).toBeHidden();

    // Turned on in Settings meanwhile: a fresh ready says so.
    pc.send({ type: 'ready', pc: PC, model: MODELS[0], models: MODELS, mayType: true });
    await pick(page, 'Dictate to PC');
    await expect(page.getByTestId('mode-hint')).toBeHidden();
    // …and the PC refusing anyway (switched off again just then): the error, back to ready.
    await page.getByTestId('talk').click();
    await expect.poll(() => pc.of('start').length).toBe(1);
    pc.send({ type: 'error', message: "This phone isn't allowed to type into your PC." });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('error')).toHaveText("This phone isn't allowed to type into your PC.");
    await expect.poll(async () => (await snapshot(page)).local).toBe('idle');
    await expect(page.getByTestId('phase')).toHaveText('Ready');
  });

  test('a dictation being typed is called off with Cancel, and its late news is dropped', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await pick(page, 'Dictate to PC');
    await ask(page, pc, 300);
    pc.send({ type: 'state', phase: 'transcribing' });
    await expect(page.getByTestId('cancel')).toBeVisible();
    await page.getByTestId('cancel').click();
    await expect.poll(() => pc.of('cancel').length).toBe(1);
    await expect(page.getByTestId('note')).toHaveText('Cancelled.');
    // What the PC had already sent on its way.
    pc.send({ type: 'heard', text: 'Too late.', to: 'pc' });
    pc.send({ type: 'pasted', ok: true, app: 'Notepad' });
    await page.waitForTimeout(300);
    await expect(page.getByTestId('result')).toBeHidden();
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('phase')).toHaveText('Ready');
  });
});
