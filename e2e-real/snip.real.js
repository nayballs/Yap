// Snip to Chat for real (src-tauri/src/snip.rs, docs/architecture/snip.md):
// the robot's own Yap captures the REAL monitors (Windows.Graphics.Capture,
// FP16 on an HDR one), its overlay comes up over them, and a real drag
// (SendInput, the helper's `snipdrag` — refused unless the robot's own
// overlay is under the pointer) snips a row of known sRGB swatches on an
// Edge page. Edge is Chromium: an HDR-aware app Windows composes at the SDR
// white level, the case an 8-bit capture washes out on an HDR monitor. The
// saved PNG must hold every swatch's colour within ±2 per channel, on every
// monitor in turn, HDR or not. And the overlay must be on screen within
// 150 ms of the request; Esc on it cancels, and the focus goes back.
//
// It takes the mouse and the focus for a few seconds per monitor: say so
// before running it while someone's at the PC.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { helper, startPage, edgeExe } from './support/targets.js';
import { readPng } from '../e2e/support/png.js';

test.use({ robotOptions: { name: 'robot-snip' } });

/** The swatches, as the page paints them. */
const SWATCHES = [
  [0x20, 0x20, 0x20],
  [0x80, 0x80, 0x80],
  [0xc0, 0xc0, 0xc0],
  [0xe8, 0xe8, 0xe8],
  [0xf5, 0xf5, 0xf5],
  [0xe5, 0x39, 0x35],
];
const SIDE = 70; // CSS px
const hex = ([r, g, b]) => `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`;

const last = (yap) => yap.invoke('e2e_snip_last');

/** An Edge page of the swatches in a row, titled with the nonce. */
async function swatchPage(robot) {
  const body = `<div id="box" tabindex="0" style="display:flex;width:max-content;margin:24px">${SWATCHES.map(
    (c) => `<div style="width:${SIDE}px;height:${SIDE}px;background:${hex(c)}"></div>`,
  ).join('')}</div>`;
  return robot.track(
    await startPage({
      nonce: robot.nonce,
      dir: path.join(robot.dir, `snip-swatches-${robot.targets.length}`),
      title: `Yap robot snip swatches - ${robot.nonce}`,
      body,
    }),
  );
}

/** Each swatch's rect on screen (physical pixels). */
async function swatchRects(page) {
  const content = helper('contentrect', page.hwnd);
  if (content.error) throw new Error(content.error);
  const { dpr, rects } = await page.evaluate(() => ({
    dpr: devicePixelRatio,
    rects: [...document.querySelectorAll('#box > div')].map((d) => {
      const r = d.getBoundingClientRect();
      return { x: r.x, y: r.y, w: r.width, h: r.height };
    }),
  }));
  return rects.map((r) => ({
    x: Math.round(content.x + r.x * dpr),
    y: Math.round(content.y + r.y * dpr),
    w: Math.round(r.w * dpr),
    h: Math.round(r.h * dpr),
  }));
}

/** The mean colour of a box's middle half in a PNG. */
function meanAt(png, x, y, w, h) {
  const [x0, y0] = [Math.round(x + w / 4), Math.round(y + h / 4)];
  const [x1, y1] = [Math.round(x + (3 * w) / 4), Math.round(y + (3 * h) / 4)];
  const sum = [0, 0, 0];
  let n = 0;
  for (let yy = y0; yy < y1; yy++) {
    for (let xx = x0; xx < x1; xx++) {
      const i = (yy * png.width + xx) * 4;
      sum[0] += png.data[i];
      sum[1] += png.data[i + 1];
      sum[2] += png.data[i + 2];
      n++;
    }
  }
  return sum.map((s) => Math.round(s / Math.max(1, n)));
}

/** Start a snip as the shortcut does; its overlay up. */
async function snipNow(yap) {
  await yap.invoke('snip_start', { origin: 'shortcut' });
  await expect.poll(async () => (await last(yap))?.outcome, { message: 'the overlay to come up' }).toBe('selecting');
  return last(yap);
}

test('a real snip of swatches on every monitor: the PNG holds their colours, HDR too, and the overlay is up within 150 ms', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is needed for the swatch page');
  const { yap } = robot;
  // The monitors, as a snip sees them (then cancelled).
  const first = await snipNow(yap);
  const screens = first.screens;
  helper('snipkey', yap.child.pid, 'escape');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  console.log(
    `snip: ${screens.length} monitor(s): ${screens.map((s) => `${s.width}x${s.height} @${s.scale}${s.hdr ? ` HDR ${s.whiteNits} nits` : ''}`).join(', ')}`,
  );

  const page = await swatchPage(robot);
  const timings = [];
  for (const [i, screen] of screens.entries()) {
    // The page on this monitor, in front.
    const placed = helper('place', page.hwnd, robot.nonce, screen.x + 80, screen.y + 120);
    expect(placed.ok, `the swatch page onto monitor ${i}`).toBe(true);
    await page.focus();
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const rects = await swatchRects(page);

    const shown = await snipNow(yap);
    timings.push(shown);
    console.log(
      `snip on monitor ${i}: captured in ${Math.round(shown.captureMs)} ms (${shown.methods.join(', ')}), overlay up in ${Math.round(shown.overlayMs)} ms, focused ${shown.focused}`,
    );
    expect(shown.overlayMs, 'the overlay within 150 ms').toBeLessThan(150);

    // A real drag across the row, a few pixels inside it.
    const [a, z] = [rects[0], rects.at(-1)];
    const drag = helper('snipdrag', yap.child.pid, a.x + 3, a.y + 3, z.x + z.w - 3, z.y + z.h - 3);
    expect(drag.ok, drag.reason ?? 'dragged').toBe(true);
    await expect.poll(async () => (await last(yap))?.outcome, { message: 'the snip saved' }).toBe('snipped');
    const { image } = await last(yap);
    const png = readPng(fs.readFileSync(path.join(yap.dataDir, 'chat-images', `${image.id}.png`)));
    expect([png.width, png.height]).toEqual([z.x + z.w - 3 - (a.x + 3), z.y + z.h - 3 - (a.y + 3)]);
    for (const [k, want] of SWATCHES.entries()) {
      const r = rects[k];
      const got = meanAt(png, r.x - (a.x + 3), r.y - (a.y + 3), r.w, r.h);
      const worst = Math.max(...got.map((v, c) => Math.abs(v - want[c])));
      expect(worst, `swatch ${hex(want)} on monitor ${i}${screen.hdr ? ' (HDR)' : ''}: read ${hex(got)}`).toBeLessThanOrEqual(2);
    }
  }
  console.log(`snip overlay timings: ${timings.map((t) => Math.round(t.overlayMs)).join(', ')} ms`);
});

test('Esc on a real snip cancels it, and the app you were in has the focus again', async ({ robot }) => {
  const { yap, pad } = robot;
  await pad.focus();
  const before = helper('foreground').hwnd;
  const shown = await snipNow(yap);
  expect(shown.focused, 'the overlay took the focus').toBe(true);
  const esc = helper('snipkey', yap.child.pid, 'escape');
  expect(esc.ok, esc.reason ?? 'escape').toBe(true);
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  expect((await last(yap)).reason).toBe('escape');
  await expect.poll(() => helper('foreground').hwnd, { message: 'the focus back' }).toBe(before);
});
