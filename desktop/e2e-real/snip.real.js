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
// Phase 2 (2026-10-25), for real: a click takes the whole monitor and
// Enter the one under the pointer (every swatch where it is on it); Space
// then a click picks the swatch page's window — exactly its visible frame
// (DWM's bounds, `yaprobot framerect`); Ctrl held as the box is let go
// copies it to the real clipboard (a PNG and a DIB, out of Win+V history),
// and Ctrl+V in Chat's box pastes it back as a screenshot, pixel for pixel.
// The robot's clipboard is saved before and put back after (robot.js).
// 2026-10-25: a plain snip (to Chat) is on the real clipboard too.
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

/** The monitors, as a snip sees them (a snip started and cancelled). */
async function monitors(yap) {
  const first = await snipNow(yap);
  helper('snipkey', yap.child.pid, 'escape');
  await yap.invoke('e2e_snip_input', { screen: 0, events: [{ kind: 'esc' }] });
  await expect.poll(async () => (await last(yap))?.outcome).toBe('cancelled');
  return first.screens;
}

/** The swatch page on `screen`, in front; its swatches' rects. */
async function pageOn(robot, page, screen) {
  const placed = helper('place', page.hwnd, robot.nonce, screen.x + 80, screen.y + 120);
  expect(placed.ok, 'the swatch page onto the monitor').toBe(true);
  await page.focus();
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  return swatchRects(page);
}

/** Every swatch's colour (±2) in a PNG whose top-left is at `origin` on screen. */
function expectSwatches(png, rects, origin, what) {
  for (const [k, want] of SWATCHES.entries()) {
    const r = rects[k];
    const got = meanAt(png, r.x - origin.x, r.y - origin.y, r.w, r.h);
    const worst = Math.max(...got.map((v, c) => Math.abs(v - want[c])));
    expect(worst, `swatch ${hex(want)} in ${what}: read ${hex(got)}`).toBeLessThanOrEqual(2);
  }
}

/** The snip just taken, once it's saved. */
async function taken(yap) {
  await expect.poll(async () => (await last(yap))?.outcome, { message: 'the snip taken' }).toBe('snipped');
  return last(yap);
}

const pngOf = (yap, image) => readPng(fs.readFileSync(path.join(yap.dataDir, 'chat-images', `${image.id}.png`)));

test('a real click takes the whole monitor, and Enter the one under the pointer', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is needed for the swatch page');
  const { yap } = robot;
  const screens = await monitors(yap);
  const page = await swatchPage(robot);
  // A click on the first monitor, away from the page.
  const s0 = screens[0];
  let rects = await pageOn(robot, page, s0);
  await snipNow(yap);
  const at = [s0.x + Math.floor(s0.width * 0.8), s0.y + Math.floor(s0.height * 0.8)];
  const clicked = helper('snipdrag', yap.child.pid, at[0], at[1], at[0], at[1]);
  expect(clicked.ok, clicked.reason ?? 'clicked').toBe(true);
  const one = await taken(yap);
  expect(one.kind).toBe('monitor');
  expect(one.rect).toEqual({ x: s0.x, y: s0.y, w: s0.width, h: s0.height });
  const png = pngOf(yap, one.image);
  expect([png.width, png.height]).toEqual([s0.width, s0.height]);
  expectSwatches(png, rects, s0, `monitor 0, whole${s0.hdr ? ' (HDR)' : ''}`);

  // Enter with the pointer on the last monitor (the other one, with two).
  const sN = screens.at(-1);
  rects = await pageOn(robot, page, sN);
  await snipNow(yap);
  const enter = helper('snipkey', yap.child.pid, 'enter', sN.x + 30, sN.y + sN.height - 30);
  expect(enter.ok, enter.reason ?? 'enter').toBe(true);
  const all = await taken(yap);
  expect(all.kind).toBe('monitor');
  expect(all.rect).toEqual({ x: sN.x, y: sN.y, w: sN.width, h: sN.height });
  expectSwatches(pngOf(yap, all.image), rects, sN, `monitor ${screens.length - 1}, by Enter${sN.hdr ? ' (HDR)' : ''}`);
});

test('Space then a real click picks the window: exactly its visible frame', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is needed for the swatch page');
  const { yap } = robot;
  const screens = await monitors(yap);
  const page = await swatchPage(robot);
  const rects = await pageOn(robot, page, screens[0]);
  const frame = helper('framerect', page.hwnd);
  expect(frame.error, 'the page window has a frame').toBeUndefined();
  // What's picked is the frame on the desktop (a window past its edge is cut there).
  const desk = screens.reduce(
    (d, s) => ({ l: Math.min(d.l, s.x), t: Math.min(d.t, s.y), r: Math.max(d.r, s.x + s.width), b: Math.max(d.b, s.y + s.height) }),
    { l: Infinity, t: Infinity, r: -Infinity, b: -Infinity },
  );
  const l = Math.max(frame.x, desk.l);
  const t = Math.max(frame.y, desk.t);
  const want = { x: l, y: t, w: Math.min(frame.x + frame.w, desk.r) - l, h: Math.min(frame.y + frame.h, desk.b) - t };

  await snipNow(yap);
  const [px, py] = [rects[1].x + 5, rects[1].y + 5];
  const space = helper('snipkey', yap.child.pid, 'space', px, py);
  expect(space.ok, space.reason ?? 'space').toBe(true);
  const clicked = helper('snipdrag', yap.child.pid, px, py, px, py);
  expect(clicked.ok, clicked.reason ?? 'clicked').toBe(true);
  const picked = await taken(yap);
  console.log(`snip: picked a ${picked.rect.w}x${picked.rect.h} window (frame ${frame.w}x${frame.h}) of ${picked.windows} on screen`);
  expect(picked.kind).toBe('window');
  expect(picked.rect).toEqual(want);
  const png = pngOf(yap, picked.image);
  expect([png.width, png.height]).toEqual([want.w, want.h]);
  expectSwatches(png, rects, want, 'the picked window');
});

test('Ctrl held as a real box is let go: copied for real, privately, and Ctrl+V pastes it into Chat', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is needed for the swatch page');
  const { yap } = robot;
  const screens = await monitors(yap);
  const page = await swatchPage(robot);
  const rects = await pageOn(robot, page, screens[0]);
  await snipNow(yap);
  const [a, z] = [rects[0], rects.at(-1)];
  const box = { x: a.x + 3, y: a.y + 3, w: z.x + z.w - 3 - (a.x + 3), h: z.y + z.h - 3 - (a.y + 3) };
  const drag = helper('snipdrag', yap.child.pid, box.x, box.y, box.x + box.w, box.y + box.h, 'ctrl');
  expect(drag.ok, drag.reason ?? 'dragged').toBe(true);
  const copied = await taken(yap);
  expect(copied.action).toBe('copy');
  expect(copied.size).toEqual([box.w, box.h]);
  expect(copied.image).toBeNull();
  // On the real clipboard: a PNG (exactly the one Yap made) and a DIB,
  // with the formats that keep it out of Win+V history.
  const check = await yap.invoke('realtest_clipboard', { action: 'checkSnip' });
  expect(check).toMatchObject({ hasPng: true, matchesSnip: true, hasDib: true, private: true, width: box.w, height: box.h });

  // Ctrl+V in Chat's box: WebView2 hands the picture over, Yap keeps it.
  await yap.invoke('e2e_main_on_screen');
  const main = yap.main;
  await main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Chat', exact: true }).click();
  const input = main.getByRole('textbox', { name: 'Message' });
  // The earlier tests' snips wait in the box: out first.
  const shots = main.getByTestId('chat-attachments');
  const remove = shots.getByRole('button', { name: 'Remove the screenshot' });
  while ((await remove.count()) > 0) await remove.first().click();
  await expect(shots).toBeHidden();
  await input.click();
  await main.keyboard.press('Control+V');
  await expect(shots.locator('.shotchip')).toHaveCount(1);
  await expect(shots).toContainText(`${box.w} × ${box.h}`);
  await expect(input).toHaveValue('');
  const src = await shots.locator('img').first().getAttribute('src');
  const id = src.match(/img_[0-9a-f]{16}/)?.[0];
  expect(id, 'the pasted picture kept as a chat screenshot').toBeTruthy();
  const png = pngOf(yap, { id });
  expect([png.width, png.height]).toEqual([box.w, box.h]);
  expectSwatches(png, rects, box, 'the pasted copy');
});

// The maintainer (2026-10-25): "When you take a screenshot it should go
// into Ctrl+V, so you can paste it anywhere". A plain snip goes to Chat AND
// onto the clipboard ("Copy every snip to the clipboard", on by default).
test('a plain real snip goes to Chat and onto the real clipboard too: the very PNG, privately', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is needed for the swatch page');
  const { yap } = robot;
  const screens = await monitors(yap);
  const page = await swatchPage(robot);
  const rects = await pageOn(robot, page, screens[0]);
  await snipNow(yap);
  const [a, z] = [rects[0], rects.at(-1)];
  const box = { x: a.x + 3, y: a.y + 3, w: z.x + z.w - 3 - (a.x + 3), h: z.y + z.h - 3 - (a.y + 3) };
  const drag = helper('snipdrag', yap.child.pid, box.x, box.y, box.x + box.w, box.y + box.h);
  expect(drag.ok, drag.reason ?? 'dragged').toBe(true);
  const snipped = await taken(yap);
  expect(snipped.action).toBe('chat');
  expect(snipped.image?.id, 'kept for Chat').toBeTruthy();
  // Copied once Chat is up, off the overlay's way.
  await expect.poll(async () => (await last(yap)).copied, { message: 'the snip on the clipboard', timeout: 5_000 }).toBe(true);
  const check = await yap.invoke('realtest_clipboard', { action: 'checkSnip' });
  expect(check).toMatchObject({ hasPng: true, matchesSnip: true, hasDib: true, private: true, width: box.w, height: box.h });
  // The clipboard's PNG is the very one Chat keeps.
  expectSwatches(pngOf(yap, snipped.image), rects, box, 'the snip in Chat');
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
