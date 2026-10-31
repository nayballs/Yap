// Yap Link's phone page against a mocked PC: pairing, Talk and its frames,
// what's shown while Yap works, the spoken answer, Stop and barge-in, the
// model chip, denied, reconnects, pings and the wake lock.
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc,
  open,
  ready,
  ask,
  snapshot,
  downFrame,
  voice,
  pitch,
  ORIGIN,
  PC,
  DEVICE,
  MODELS,
} from './support/harness.js';
import { TONE_HZ } from './support/audio.js';

test.describe('pairing', () => {
  test('from the QR: the code shown big, the device kept, the fragment cleared; a later visit says hello with it', async ({
    page,
  }) => {
    const served = await servePage(page);
    const pc = await mockPc(page, {
      onHello: (hello, conn, pc) => {
        if (hello.pair) pc.sendTo(conn, { type: 'pair_wait', code: '4821' });
        else if (hello.device) pc.ready(conn);
      },
    });
    await open(page, '#pair=tok-abc123');

    await expect.poll(() => pc.of('hello').length).toBe(1);
    const hello = pc.of('hello')[0];
    expect(hello.v).toBe(1);
    expect(hello.pair).toEqual({ token: 'tok-abc123' });
    expect(hello.device).toBeUndefined();
    expect(hello.name).toMatch(/^\S.* · \S/);
    expect(hello.nonce).toMatch(/^[A-Za-z0-9_-]{16,}$/);

    await expect(page.getByTestId('pair-code')).toHaveText('4821');
    await expect(page.getByTestId('pair-code-box')).toContainText('Check this code on your PC:');
    await expect(page.getByTestId('pair-code-box')).toContainText('then press Allow');
    await expect(page.getByTestId('talk')).toBeHidden();

    pc.send({ type: 'paired', device: DEVICE });
    pc.ready();
    await ready(page, pc);
    await expect(page.getByTestId('pair-code')).toBeHidden();
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.device')))).toEqual(DEVICE);
    expect(await page.evaluate(() => location.hash)).toBe('');
    expect(page.url()).toBe(`${ORIGIN}/`);

    // Back later: hello with the device, no pairing.
    await open(page);
    await expect.poll(() => pc.of('hello').length).toBe(2);
    const again = pc.of('hello')[1];
    expect(again.device).toEqual(DEVICE);
    expect(again.pair).toBeUndefined();
    expect(again.nonce).not.toBe(hello.nonce);
    await ready(page, pc);

    // Nothing from anywhere but the PC.
    expect(served.filter((s) => s.outside || s.missing)).toEqual([]);
    expect(served.map((s) => s.file)).toEqual(expect.arrayContaining(['index.html', 'link.css', 'link.js', 'dsp.js', 'icon.svg']));
    // Add to Home screen: the manifest and its icon are there.
    const manifest = await page.evaluate(async () => {
      const href = document.querySelector('link[rel=manifest]').href;
      return (await fetch(href)).json();
    });
    expect(manifest).toMatchObject({ name: 'Yap Link', start_url: './', display: 'standalone' });
    expect(manifest.icons[0].src).toBe('icon.svg');
  });

  test('no device and no code: how to pair, and no connection tried', async ({ page }) => {
    await servePage(page);
    const pc = await mockPc(page);
    await open(page);
    await expect(page.getByTestId('pair-how')).toBeVisible();
    await expect(page.getByTestId('pair-how')).toContainText('In Yap on your PC, open Settings → Phone → Pair a phone');
    await expect(page.getByTestId('conn')).toHaveText('Not paired');
    await page.waitForTimeout(800);
    expect(pc.conns.length).toBe(0);
  });

  test('denied: how to pair, said plainly, and no reconnecting', async ({ page }) => {
    await servePage(page);
    await seedDevice(page);
    const pc = await mockPc(page, {
      onHello: (hello, conn, pc) => {
        pc.sendTo(conn, { type: 'denied', reason: 'revoked' });
        conn.ws.close();
      },
    });
    await open(page);
    await expect(page.getByTestId('pair-why')).toHaveText("This phone isn't paired with Yap on your PC any more.");
    await expect(page.getByTestId('pair-how')).toContainText('Settings → Phone → Pair a phone');
    await expect(page.getByTestId('conn')).toHaveText('Not paired');
    await page.waitForTimeout(2000);
    expect(pc.conns.length).toBe(1);
    // Try again: once more, on purpose.
    await page.getByRole('button', { name: 'Try again' }).click();
    await expect.poll(() => pc.conns.length).toBe(2);
  });
});

test.describe('connection', () => {
  test('a dropped connection: "Reconnecting to Office PC…", then back with the same device', async ({ page }) => {
    await servePage(page);
    await seedDevice(page);
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    pc.close();
    await expect(page.getByTestId('conn')).toHaveText(`Reconnecting to ${PC}…`);
    await expect(page.getByTestId('talk')).toBeDisabled();
    await expect.poll(() => pc.conns.length, { timeout: 5000 }).toBe(2);
    expect(pc.conns[1].texts[0]).toMatchObject({ type: 'hello', v: 1, device: DEVICE });
    await ready(page, pc);
  });

  test('pings keep it alive; no answer at all and it reconnects', async ({ page }) => {
    await page.clock.install();
    await servePage(page);
    await seedDevice(page);
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await page.clock.pauseAt(Date.now() + 500);
    // The page's time, in steps, until `cond` holds: how long it took.
    const conn = page.getByTestId('conn');
    const until = async (cond, max = 20_000) => {
      for (let t = 0; t <= max; t += 200) {
        await page.waitForTimeout(20); // the socket's messages land
        if (await cond()) return t;
        await page.clock.runFor(200);
      }
      throw new Error('never happened');
    };
    const first = await until(() => pc.of('ping').length === 1);
    expect(first).toBeLessThanOrEqual(15_200);
    const second = await until(() => pc.of('ping').length === 2);
    expect(second).toBeGreaterThan(14_400);
    expect(second).toBeLessThanOrEqual(15_400);
    // Answered: still the one connection.
    expect(pc.conns.length).toBe(1);
    // The PC goes quiet: no pong.
    pc.autoPong = false;
    await until(() => pc.of('ping').length === 3);
    const dead = await until(async () => (await conn.textContent()) === `Reconnecting to ${PC}…`);
    expect(dead).toBeGreaterThan(7_400);
    expect(dead).toBeLessThanOrEqual(8_600);
    await until(() => pc.conns.length === 2, 2_000);
    expect(pc.conns[1].texts[0]).toMatchObject({ type: 'hello', device: DEVICE });
    await expect(conn).toHaveText(`Connected to ${PC}`);
  });

  test('the screen is kept awake, and the lock taken again when the page comes back', async ({ page }) => {
    await page.addInitScript(() => {
      // A wake lock that can be watched (headless has no screen to keep on).
      window.__locks = [];
      Object.defineProperty(navigator, 'wakeLock', {
        configurable: true,
        value: {
          request: async (type) => {
            const s = new EventTarget();
            s.type = type;
            s.released = false;
            s.release = async () => {
              s.released = true;
              s.dispatchEvent(new Event('release'));
            };
            window.__locks.push(s);
            return s;
          },
        },
      });
    });
    await servePage(page);
    await seedDevice(page);
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await expect.poll(() => page.evaluate(() => window.__locks.length)).toBeGreaterThan(0);
    expect(await page.evaluate(() => window.__locks[0].type)).toBe('screen');
    expect((await snapshot(page)).wakeLock).toBe(true);
    // The system lets it go (the page was hidden); back on screen, it's taken again.
    await page.evaluate(() => window.__locks.at(-1).release());
    expect((await snapshot(page)).wakeLock).toBe(false);
    await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
    await expect.poll(async () => (await snapshot(page)).wakeLock).toBe(true);
    expect(await page.evaluate(() => window.__locks.length)).toBe(2);
    await expect(page.getByText('Keep this page open. Your headset works as the mic and speaker.')).toBeVisible();
  });
});

test.describe('talking', () => {
  test.beforeEach(async ({ page }) => {
    await servePage(page);
    await seedDevice(page);
  });

  test('Talk: start, then 20 ms frames of 16 kHz Int16 from the mic, then stop', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await page.getByTestId('talk').click();
    await expect(page.getByTestId('phase')).toHaveText('Listening');
    await expect(page.getByTestId('meter')).toBeVisible();
    await expect.poll(() => pc.frames.length, { timeout: 5000 }).toBeGreaterThan(60);
    await expect(page.getByTestId('talk-time')).toHaveText(/^0:0[1-9]$/);
    await page.getByTestId('talk').click();
    await expect.poll(() => pc.of('stop').length).toBe(1);
    await expect(page.getByTestId('phase')).toHaveText('Working it out');

    // start first, every frame between start and stop, stop last.
    const order = pc.log.map((e) => (e.text ? e.text.type : 'frame')).filter((t) => t !== 'ping');
    const first = order.indexOf('start');
    const last = order.lastIndexOf('frame');
    expect(pc.of('start')[0]).toEqual({ type: 'start', mode: 'ask' });
    expect(order.indexOf('frame')).toBeGreaterThan(first);
    expect(order.indexOf('stop')).toBeGreaterThan(last);

    // Kind 0x01, seq from 0 counting up, 320 samples (20 ms at 16 kHz) each.
    const frames = pc.frames;
    frames.forEach((f, i) => {
      expect(f.kind).toBe(0x01);
      expect(f.seq).toBe(i);
      expect(f.bytes).toBe(5 + 640);
    });
    // The rate: the mic's 440 Hz tone is 440 Hz at 16 kHz.
    const all = new Int16Array(frames.length * 320);
    frames.forEach((f, i) => all.set(f.samples, i * 320));
    const steady = all.subarray(16000 * 0.3, all.length - 640);
    expect(Math.abs(pitch(steady, 16000) - TONE_HZ)).toBeLessThan(TONE_HZ * 0.03);
    const snap = await snapshot(page);
    expect([44100, 48000]).toContain(snap.contextRate);
  });

  test('hold to talk: release sends', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    const box = await page.getByTestId('talk').boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await expect.poll(() => pc.of('start').length).toBe(1);
    await expect(page.getByTestId('talk')).toContainText('Release to send');
    await page.waitForTimeout(700);
    expect(pc.of('stop').length).toBe(0);
    await page.mouse.up();
    await expect.poll(() => pc.of('stop').length).toBe(1);
    expect(pc.frames.length).toBeGreaterThan(25);
    await page.waitForTimeout(400);
    // The release's click doesn't start another question.
    expect(pc.of('start').length).toBe(1);
    expect((await snapshot(page)).local).toBe('idle');
  });

  test('what was heard, the step with its time, and the answer as it streams', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc);
    pc.send({ type: 'state', phase: 'transcribing' });
    pc.send({ type: 'heard', text: 'When is high tide at the harbour today?' });
    pc.send({ type: 'state', phase: 'thinking' });
    await expect(page.getByTestId('heard').last()).toHaveText('When is high tide at the harbour today?');
    await expect(page.getByTestId('phase')).toHaveText('Working it out');
    await expect(page.getByTestId('step')).toContainText('Thinking');
    pc.send({ type: 'step', label: 'Searching the web · harbour tide times' });
    await expect(page.getByTestId('step')).toContainText('Searching the web · harbour tide times');
    await expect(page.locator('#step-time')).toHaveText(/^ · [1-9]s$/, { timeout: 4000 });
    pc.send({ type: 'answer', text: 'High tide is at', final: false });
    await expect(page.getByTestId('answer').last()).toHaveText('High tide is at');
    pc.send({ type: 'answer', text: 'High tide is at 4:12 pm, **3.9 metres** [1].', final: false });
    await expect(page.getByTestId('answer').last()).toHaveText('High tide is at 4:12 pm, 3.9 metres.');
    pc.send({ type: 'answer', text: 'High tide is at 4:12 pm, 3.9 metres.', final: true });
    await expect(page.getByTestId('answer').last()).toHaveText('High tide is at 4:12 pm, 3.9 metres.');
    await expect(page.getByTestId('answer').last()).toHaveClass(/final/);
    await expect(page.getByTestId('step')).toBeHidden();
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('phase')).toHaveText('Ready');
    await expect(page.getByTestId('turn')).toHaveCount(1);
  });

  test("errors in plain words, and the next question starts clean", async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc);
    pc.send({ type: 'error', message: "Yap's Voice Assistant is off while a meeting records." });
    pc.send({ type: 'state', phase: 'idle' });
    await expect(page.getByTestId('error')).toHaveText("Yap's Voice Assistant is off while a meeting records.");
    await expect(page.getByRole('alert')).toBeVisible();
    await page.getByTestId('talk').click();
    await expect(page.getByTestId('error')).toBeHidden();
  });

  test("the answer spoken: one stream at the phone's own rate, played out to the end", async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc);
    pc.send({ type: 'state', phase: 'speaking' });
    pc.send({ type: 'speak_start', rate: 24000 });
    for (let i = 0; i < 6; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
    await expect.poll(async () => (await snapshot(page)).player.queued).toBeCloseTo(0.6, 6);
    // 600 ms waiting is past the 250 ms the buffer wants: it plays straight away.
    await expect.poll(async () => (await snapshot(page)).player.state).toBe('playing');
    await expect(page.getByTestId('phase')).toHaveText('Speaking');
    await expect(page.getByTestId('stop')).toBeVisible();
    let p = (await snapshot(page)).player;
    expect(p.rate).toBe(24000);
    expect(p.loaded).toBe(true);
    expect(p.startBufferedMs).toBeGreaterThanOrEqual(250);
    expect(p.startDelayMs).toBeLessThan(100);
    pc.send({ type: 'speak_end' });
    pc.send({ type: 'state', phase: 'idle' });
    // 0.6 s of sound, then quiet: all of it played, none of it missing.
    await expect.poll(async () => (await snapshot(page)).player.active, { timeout: 4000 }).toBe(false);
    p = (await snapshot(page)).player;
    expect(p.state).toBe('idle');
    expect(p.played).toBeCloseTo(0.6, 3);
    expect(p.underruns).toBe(0);
    expect(p.maxStep).toBeLessThan(0.03);
    await expect(page.getByTestId('stop')).toBeHidden();
    await expect(page.getByTestId('phase')).toHaveText('Ready');
    expect((await snapshot(page)).contextState).toBe('running');
  });

  test('Stop: interrupt sent, the answer silent at once (a fade, not a cut), its late frames dropped', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc);
    pc.send({ type: 'state', phase: 'speaking' });
    pc.send({ type: 'speak_start', rate: 24000 });
    for (let i = 0; i < 20; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
    await expect.poll(async () => (await snapshot(page)).player.queued).toBeCloseTo(2, 6);
    await expect.poll(async () => (await snapshot(page)).player.played).toBeGreaterThan(0.2);
    await page.getByTestId('stop').click();
    await expect.poll(() => pc.of('interrupt').length).toBe(1);
    let p = (await snapshot(page)).player;
    expect(p.active).toBe(false);
    await expect(page.getByTestId('stop')).toBeHidden();
    // In the worklet: faded to silence, the rest thrown away.
    await expect.poll(async () => (await snapshot(page)).player.state).toBe('idle');
    await expect.poll(async () => (await snapshot(page)).player.level).toBe(0);
    p = (await snapshot(page)).player;
    expect(p.buffered).toBe(0);
    expect(p.played).toBeLessThan(1.5);
    expect(p.maxStep).toBeLessThan(0.03);
    const played = p.played;
    const queued = p.queued;
    // Already on the way when Stop was pressed.
    for (let i = 20; i < 24; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
    pc.send({ type: 'speak_end' });
    pc.send({ type: 'state', phase: 'idle' });
    await page.waitForTimeout(300);
    p = (await snapshot(page)).player;
    expect(p.queued).toBe(queued);
    expect(p.played).toBe(played);
    expect(p.active).toBe(false);
    await expect(page.getByTestId('phase')).toHaveText('Ready');
  });

  test('Talk while it speaks: interrupt, then a new question (barge-in)', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc);
    pc.send({ type: 'state', phase: 'speaking' });
    pc.send({ type: 'speak_start', rate: 24000 });
    for (let i = 0; i < 30; i++) pc.sendBinary(downFrame(i, voice(24000, 100)));
    await expect(page.getByTestId('stop')).toBeVisible();
    const framesBefore = pc.frames.length;
    await page.getByTestId('talk').click();
    await expect.poll(() => pc.of('start').length).toBe(2);
    const types = pc.types().filter((t) => t !== 'ping');
    expect(types.slice(-2)).toEqual(['interrupt', 'start']);
    expect((await snapshot(page)).player.active).toBe(false);
    await expect.poll(async () => (await snapshot(page)).player.state).toBe('idle');
    await expect(page.getByTestId('phase')).toHaveText('Listening');
    pc.send({ type: 'state', phase: 'idle' }); // the interrupt's
    pc.send({ type: 'state', phase: 'listening' }); // the new question's
    await expect.poll(() => pc.frames.length - framesBefore).toBeGreaterThan(10);
    // The new question's frames count from 0 again.
    expect(pc.frames[framesBefore].seq).toBe(0);
    await expect(page.getByTestId('phase')).toHaveText('Listening');
  });

  test('Cancel while listening drops the question', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await page.getByTestId('talk').click();
    await expect.poll(() => pc.frames.length).toBeGreaterThan(5);
    await page.getByTestId('cancel').click();
    await expect.poll(() => pc.of('cancel').length).toBe(1);
    expect(pc.of('stop').length).toBe(0);
    const n = pc.frames.length;
    await page.waitForTimeout(300);
    expect(pc.frames.length).toBe(n);
    await expect(page.getByTestId('phase')).toHaveText('Ready');
    await expect(page.getByTestId('turn')).toHaveCount(0);
  });

  test('the model chip: "On this PC", the list, and set_model', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    const chip = page.getByTestId('model-chip');
    await expect(chip).toContainText('Qwen3.5 4B');
    await expect(chip).toContainText('On this PC');
    // Said once: the PC's label is the name alone, the tag says where.
    expect((await chip.textContent()).match(/On this PC/g)).toHaveLength(1);
    await chip.click();
    const sheet = page.getByTestId('models');
    await expect(sheet).toBeVisible();
    await expect(sheet.getByRole('button', { name: /Qwen3.5 4B/ })).toHaveAttribute('aria-current', 'true');
    await expect(sheet.getByRole('button', { name: /Claude Haiku/ })).toContainText('Cloud');
    await sheet.getByRole('button', { name: /Claude Haiku/ }).click();
    await expect.poll(() => pc.of('set_model')).toEqual([{ type: 'set_model', id: MODELS[1].id }]);
    await expect(sheet).toBeHidden();
    await expect(chip).toContainText('Claude Haiku');
    await expect(chip).not.toContainText('On this PC');
    // The PC's own word wins.
    pc.send({ type: 'ready', pc: PC, model: MODELS[0], models: MODELS });
    await expect(chip).toContainText('Qwen3.5 4B');
  });

  test('the microphone picker lists the mics and remembers the choice', async ({ page }) => {
    const pc = await mockPc(page);
    await open(page);
    await ready(page, pc);
    await ask(page, pc, 200);
    const select = page.getByTestId('mic');
    await expect.poll(async () => (await select.locator('option').count())).toBeGreaterThan(1);
    const second = await select.locator('option').nth(1).getAttribute('value');
    await select.selectOption(second);
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.mic')).id)).toBe(second);
    await open(page);
    await ready(page, pc);
    await expect(page.getByTestId('mic')).toHaveValue(second);
  });
});
