// Computer use, phase 1, on the REAL screen (desktop/src-tauri/src/computer/shot.rs,
// computer/windows.rs, docs/architecture/computer-use.md): with only the robot's test pad
// allowed (its exe, yaprobot.exe, which nothing else runs), a screenshot keeps the pad's own
// pixels and masks every other window — an Edge page over part of it, and whatever else is on
// the display — in grey, as the model would get it. The capture is Snip's (Windows.Graphics.Capture),
// the windows Windows' own list (DWM frames, z-order), so this is what the stand-in desktop
// can't show: real windows told apart by their process, on a real display.
//
// No input is sent: it only places the robot's own windows and captures. Robot mode still,
// since it captures the real screen (only the robot's windows are left unmasked).
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { helper, startPage, edgeExe, HELPER } from './support/targets.js';
import { readPng } from '../e2e/support/png.js';

test.use({ robotOptions: { name: 'robot-computer' } });

/** A masked window's grey, as computer/shot.rs paints it. */
const MASK_FILL = [62, 62, 68];

function pixel(png, x, y) {
  const i = (y * png.width + x) * 4;
  return [png.data[i], png.data[i + 1], png.data[i + 2]];
}

test('a screenshot keeps the allowed app and masks the window over it', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is needed for the window over the pad');
  const { yap, pad } = robot;
  const page = robot.track(
    await startPage({ nonce: robot.nonce, dir: path.join(robot.dir, 'computer-page'), title: `Yap robot computer page - ${robot.nonce}`, body: '<div style="height:100vh;background:#e53935"></div>' }),
  );
  // The pad, then the page over its right half.
  const padAt = helper('place', pad.hwnd, robot.nonce, 120, 120);
  expect(padAt.ok, 'the pad placed').toBe(true);
  await pad.focus();
  const padFrame = helper('framerect', pad.hwnd);
  const pageAt = helper('place', page.hwnd, robot.nonce, padFrame.x + Math.round(padFrame.w / 2), padFrame.y + 40);
  expect(pageAt.ok, 'the page placed').toBe(true);
  await page.focus();
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const pageFrame = helper('framerect', page.hwnd);

  const out = path.join(robot.dir, 'computer-look.png');
  const got = await yap.invoke('e2e_computer_look', { exes: [HELPER], out });
  console.log(`computer use: ${got.display} ${got.rect.w}x${got.rect.h}, frame ${got.frame.w}x${got.frame.h}, ${got.shown} allowed window(s), ${got.masked.length} masked`);
  expect(got.shown, 'the pad is the one allowed window').toBe(1);
  expect(got.masked.length, 'the page, at least, is masked').toBeGreaterThanOrEqual(1);
  const png = readPng(fs.readFileSync(out));
  expect([png.width, png.height]).toEqual([got.rect.w, got.rect.h]);
  const local = (x, y) => [x - got.rect.x, y - got.rect.y];

  // The pad's left half, nothing over it: its own pixels (a WinForms window, never the mask's grey).
  const [px, py] = local(padFrame.x + 30, padFrame.y + Math.round(padFrame.h / 2));
  expect(pixel(png, px, py), "the pad's own pixels").not.toEqual(MASK_FILL);
  // The page over the pad: masked, its red never shown.
  const [qx, qy] = local(pageFrame.x + Math.round(pageFrame.w / 2), pageFrame.y + Math.round(pageFrame.h / 2));
  expect(pixel(png, qx, qy), 'the page masked').toEqual(MASK_FILL);
  const reds = [];
  for (let y = 0; y < png.height; y += 7) for (let x = 0; x < png.width; x += 7) if (pixel(png, x, y).join() === '229,57,53') reds.push([x, y]);
  expect(reds, "nothing of the page's red anywhere").toEqual([]);
  // The robot's own Yap windows are Yap's, never shown.
  const yapMasked = got.masked.filter((m) => m.what === 'Yap');
  console.log(`computer use: ${yapMasked.length} of Yap's own window(s) on that display, masked`);
  fs.rmSync(out, { force: true });
});
