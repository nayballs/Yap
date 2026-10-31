// Opening the mic never waits silently: each step of it (link.js doOpenMic)
// has a time limit and a reason on screen when it runs out, Cancel always
// leaves Talk ready, and Sound details says which step it's on. Yap on the
// PC mocked; the mic the looped 440 Hz tone, held back here where a test
// needs a permission nobody answers.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, snapshot } from './support/harness.js';

const PERMISSION_WAIT =
  'Waiting for the microphone permission. Tap the icon left of the address bar → Microphone → Allow, then tap Talk again.';
const PERMISSION_BLOCKED =
  'The microphone is blocked for this page. Tap the icon left of the address bar → Microphone → Allow, then tap Talk again.';
const SETUP = "The audio setup didn't finish: reload the page, then try again.";
const NO_SOUND = "The phone's sound didn't start. Tap Talk again, or reload the page.";

/**
 * getUserMedia held back while `window.__mic.hold`, as a permission prompt
 * nobody answers; `__mic.release()` lets the calls waiting go on. Every
 * track it hands out is kept, to see them stop.
 */
async function holdMic(page) {
  await page.addInitScript(() => {
    const md = navigator.mediaDevices;
    const real = md.getUserMedia.bind(md);
    const m = (window.__mic = { hold: true, calls: 0, waiting: [], tracks: [] });
    const get = (c) =>
      real(c).then((s) => {
        m.tracks.push(...s.getAudioTracks());
        return s;
      });
    md.getUserMedia = (c) => {
      m.calls++;
      if (!m.hold) return get(c);
      return new Promise((resolve, reject) => m.waiting.push(() => get(c).then(resolve, reject)));
    };
    m.release = () => {
      m.hold = false;
      for (const go of m.waiting.splice(0)) go();
    };
    m.live = () => m.tracks.filter((t) => t.readyState === 'live').length;
  });
}

const mic = (page, expr) => page.evaluate(`window.__mic.${expr}`);

/** Talk tapped: the mic starts opening. */
async function tapTalk(page) {
  await page.getByTestId('talk').click();
}

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

test('a permission nobody answers: after ~6 s how to allow it, with Cancel; Cancel leaves Talk ready', async ({ page }) => {
  await holdMic(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByText('Sound details').click();
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Opening the microphone…');
  await expect(page.getByTestId('talk')).toContainText('Wait…');
  await expect(page.getByTestId('cancel')).toBeVisible();
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Asking for the microphone… [\d,.]+ m?s so far$/);
  // Not said too soon…
  await page.waitForTimeout(4000);
  await expect(page.getByTestId('error')).toBeHidden();
  // …then said plainly, Talk ready to try again, Cancel still there.
  await expect(page.getByTestId('error')).toHaveText(PERMISSION_WAIT, { timeout: 4000 });
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.getByTestId('talk')).toContainText('Talk');
  await expect(page.getByTestId('cancel')).toBeVisible();
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Asking for the microphone… [6-9]\.\d s so far$/);
  expect((await snapshot(page)).micStep).toMatchObject({ name: 'permission', state: 'now', waiting: true });
  // Nothing went to the PC: no question was asked.
  expect(pc.of('start')).toHaveLength(0);

  // Cancel: back to Talk, the message gone, the step said cancelled.
  await page.getByTestId('cancel').click();
  await expect(page.getByTestId('error')).toBeHidden();
  await expect(page.getByTestId('talk')).toContainText('Talk');
  await expect(page.getByTestId('cancel')).toBeHidden();
  await expect(page.getByTestId('phase')).toHaveText('Ready');
  let snap = await snapshot(page);
  expect(snap.local).toBe('idle');
  expect(snap.micStep).toMatchObject({ name: 'permission', state: 'cancelled' });
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Asking for the microphone: cancelled after [6-9]\.\d s$/);

  // The prompt answered after all: that late mic goes straight back.
  await mic(page, 'release()');
  await expect.poll(() => mic(page, 'tracks.length')).toBe(1);
  await expect.poll(() => mic(page, 'live()')).toBe(0);
  snap = await snapshot(page);
  expect(snap.micOpen).toBe(false);
  expect(snap.local).toBe('idle');

  // And Talk works.
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await expect.poll(() => pc.of('start').length).toBe(1);
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
  expect(await mic(page, 'calls')).toBe(2);
});

test('the permission answered late: the question goes ahead, the message gone', async ({ page }) => {
  await holdMic(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await tapTalk(page);
  await expect(page.getByTestId('error')).toHaveText(PERMISSION_WAIT, { timeout: 9000 });
  await mic(page, 'release()');
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await expect(page.getByTestId('error')).toBeHidden();
  await expect.poll(() => pc.of('start').length).toBe(1);
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
  await tapTalk(page);
  await expect.poll(() => pc.of('stop').length).toBe(1);
  expect(await mic(page, 'calls')).toBe(1);
});

test('stuck at the permission, then allowed from the address bar: Talk tries afresh', async ({ page }) => {
  await holdMic(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await tapTalk(page);
  await expect(page.getByTestId('error')).toHaveText(PERMISSION_WAIT, { timeout: 9000 });
  // Allowed from the site's settings: the first ask still waits, a new one doesn't.
  await page.evaluate(() => (window.__mic.hold = false));
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await expect(page.getByTestId('error')).toBeHidden();
  expect(await mic(page, 'calls')).toBe(2);
  await expect.poll(() => pc.of('start').length).toBe(1);
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
});

test('the microphone blocked: said at once, where the browser says so', async ({ page }) => {
  await page.addInitScript(() => {
    // As a browser that knows the page was refused the mic.
    const q = navigator.permissions.query.bind(navigator.permissions);
    navigator.permissions.query = (d) => (d && d.name === 'microphone' ? Promise.resolve({ state: 'denied' }) : q(d));
    navigator.mediaDevices.getUserMedia = () => Promise.reject(new DOMException('Permission denied', 'NotAllowedError'));
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await tapTalk(page);
  await expect(page.getByTestId('error')).toHaveText(PERMISSION_BLOCKED, { timeout: 2000 });
  await expect(page.getByTestId('talk')).toContainText('Talk');
  await expect(page.getByTestId('cancel')).toBeHidden();
  expect((await snapshot(page)).local).toBe('idle');
  expect(pc.of('start')).toHaveLength(0);
});

test('a worklet module that never loads: the setup message, one module at a time, and Talk works after', async ({ page }) => {
  await page.addInitScript(() => {
    // After the harness's own addModule (support/harness.js): held back
    // while __modules.hang, counted, and how many at once.
    const prev = AudioWorklet.prototype.addModule;
    const m = (window.__modules = { hang: true, calls: [], inFlight: 0, most: 0 });
    AudioWorklet.prototype.addModule = function (url, options) {
      m.calls.push(String(url).split('/').pop());
      if (m.hang) return new Promise(() => {});
      m.inFlight++;
      m.most = Math.max(m.most, m.inFlight);
      return prev.call(this, url, options).finally(() => m.inFlight--);
    };
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByText('Sound details').click();
  await tapTalk(page);
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Loading the recorder… /);
  await expect(page.getByTestId('error')).toHaveText(SETUP, { timeout: 8000 });
  await expect(page.getByTestId('talk')).toContainText('Talk');
  await expect(page.getByTestId('cancel')).toBeHidden();
  expect((await snapshot(page)).local).toBe('idle');
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Loading the recorder: stopped after 5\.\d s$/);
  // The player's module never started beside the stuck one.
  expect(await page.evaluate(() => window.__modules.calls)).toEqual(['mic-worklet.js']);

  // Loading works again: Talk sets the sound up afresh, a module at a time.
  await page.evaluate(() => {
    window.__modules.hang = false;
    window.__modules.calls = [];
  });
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await expect(page.getByTestId('error')).toBeHidden();
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
  const m = await page.evaluate(() => window.__modules);
  expect(m.calls).toEqual(['mic-worklet.js', 'player-worklet.js']);
  expect(m.most).toBe(1);
  expect((await snapshot(page)).player.loaded).toBe(true);
});

test("the phone's sound never starts: said after a few seconds, and the mic let go", async ({ page }) => {
  await page.addInitScript(() => {
    // An AudioContext that stays suspended whatever it's asked, while __stuck.
    window.__stuck = true;
    const state = Object.getOwnPropertyDescriptor(BaseAudioContext.prototype, 'state');
    Object.defineProperty(BaseAudioContext.prototype, 'state', {
      configurable: true,
      get() {
        return window.__stuck ? 'suspended' : state.get.call(this);
      },
    });
    // resume() is AudioContext's (the spec), or BaseAudioContext's (older engines).
    const owner = Object.prototype.hasOwnProperty.call(AudioContext.prototype, 'resume')
      ? AudioContext.prototype
      : BaseAudioContext.prototype;
    const resume = owner.resume;
    owner.resume = function () {
      return window.__stuck ? new Promise(() => {}) : resume.call(this);
    };
    const md = navigator.mediaDevices;
    const real = md.getUserMedia.bind(md);
    window.__tracks = [];
    md.getUserMedia = async (c) => {
      const s = await real(c);
      window.__tracks.push(...s.getAudioTracks());
      return s;
    };
  });
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByText('Sound details').click();
  await tapTalk(page);
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Starting the phone's sound… /);
  await expect(page.getByTestId('error')).toHaveText(NO_SOUND, { timeout: 6000 });
  await expect(page.getByTestId('talk')).toContainText('Talk');
  expect((await snapshot(page)).local).toBe('idle');
  await expect(page.getByTestId('sound-mic-step')).toHaveText(/^Starting the phone's sound: stopped after 3\.\d s$/);
  // The mic it had opened is let go.
  expect(await page.evaluate(() => window.__tracks.filter((t) => t.readyState === 'live').length)).toBe(0);
  expect(pc.of('start')).toHaveLength(0);

  // The sound starts after all: Talk works.
  await page.evaluate(() => (window.__stuck = false));
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await expect(page.getByTestId('error')).toBeHidden();
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
});

test('Cancel while the mic opens, at any step, leaves Talk ready', async ({ page }) => {
  await holdMic(page);
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Opening the microphone…');
  await page.getByTestId('cancel').click();
  await expect(page.getByTestId('talk')).toContainText('Talk');
  await expect(page.getByTestId('phase')).toHaveText('Ready');
  // Whichever step it was on (the modules, or asking for the mic).
  expect((await snapshot(page)).micStep).toMatchObject({ state: 'cancelled' });
  const asked = await mic(page, 'calls');
  expect(asked).toBeLessThanOrEqual(1);
  // The mic allowed now: the next Talk asks afresh and listens.
  await mic(page, 'release()');
  await tapTalk(page);
  await expect(page.getByTestId('phase')).toHaveText('Listening');
  await expect.poll(() => pc.frames.length).toBeGreaterThan(10);
  expect(await mic(page, 'calls')).toBe(asked + 1);
  expect(pc.of('cancel')).toHaveLength(0); // nothing had gone to the PC
});
