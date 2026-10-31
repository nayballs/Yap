// Chat's screenshots as the Claude app draws them — the tests, run at a
// display scaling (chat-shots.spec.js at 100 %, chat-shots-150.spec.js at a
// real 150 %: WebView2 drawn at that device scale factor, as on a 150 %
// monitor; CDP's emulation lays a page out at 100 % and scales it, which a
// 150 % screen doesn't). See chat-shots.spec.js for what they check.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './fixtures.js';
import { startFakeLlm } from './fake-llm.js';
import { SCREENSHOTS, WEBVIEW2_ARGS } from './yap.js';
import { readPng, pixel, near } from './png.js';
import { screenshotLike, encode, crop, areaAverage, compare, enlarge, sideBySide } from './sharpness.js';

const TEST_KEY = 'e2e-fake-key';
/** A run's name in the measurements' files ("before" against an older build: YAP_E2E_EXE). */
const LABEL = process.env.SHOTS_LABEL || 'after';
const OUT = path.join(SCREENSHOTS, 'chat-shots');

/** The made-up screenshot of a dark editor (support/sharpness.js). */
const SOURCE = screenshotLike(1920, 1080);
/** A small one-colour snip: one image pixel per device pixel wherever it goes. */
const SMALL = flat(200, 100, [0x3c, 0x8d, 0xd9]);

/** The frame's fill and the ring laid over it (app.css `--yap-shot-*`), per theme. */
const LOOK = {
  light: { fill: [255, 255, 255], ring: [35, 33, 27], alpha: 0.16 },
  dark: { fill: [0x12, 0x10, 0x0d], ring: [255, 255, 255], alpha: 0.2 },
};

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const attachments = (main) => main.getByTestId('chat-attachments');

/** A one-colour picture. */
function flat(width, height, [r, g, b]) {
  const data = Buffer.alloc(width * height * 4);
  for (let i = 0; i < width * height; i++) data.set([r, g, b, 255], i * 4);
  return { width, height, data };
}

/** `w` × `h` contained in a box, never enlarged (snip/thumbs.rs `contain`). */
function contain(w, h, bw, bh) {
  const s = Math.min(bw / w, bh / h, 1);
  return [Math.max(1, Math.round(w * s)), Math.max(1, Math.round(h * s))];
}

/** The ring over `fill`, as one device pixel shows it. */
function ringOver(theme) {
  const { fill, ring, alpha } = LOOK[theme];
  return fill.map((c, i) => Math.round(c + alpha * (ring[i] - c)));
}

/** A picture file dropped on Chat (snip/attach.rs: it waits in the box as a snip does). */
async function dropPicture(yap, name, img) {
  const dir = path.join(yap.dataDir, 'to-drop');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, name);
  fs.writeFileSync(file, encode(img));
  await yap.invoke('e2e_drag_drop', { paths: [file] });
}

/** A picture dropped, waiting in the box (`count` of them there now). */
async function drop(main, yap, name, img, count) {
  await dropPicture(yap, name, img);
  await expect(attachments(main).locator('.shotchip')).toHaveCount(count);
}

/** A new chat, its box empty. */
async function freshChat(main) {
  await openView(main, 'Chat');
  // (A narrow window folds the conversation list away: the chat open then.)
  const fresh = main.getByRole('button', { name: 'New chat' }).first();
  if (await fresh.isVisible()) await fresh.click();
  const remove = attachments(main).getByRole('button', { name: 'Remove the screenshot' });
  while ((await remove.count()) > 0) await remove.first().click();
  await expect(attachments(main)).toBeHidden();
}

/** A question sent (the fake answers "OK"), its screenshots drawn. */
async function ask(main, fakeLlm, text) {
  const before = fakeLlm.requests.length;
  await box(main).fill(text);
  await box(main).press('Enter');
  const bubble = main.locator('.cbubble.user', { hasText: text }).last();
  await expect(bubble.locator('.shots img').first()).toBeVisible();
  await expect.poll(() => fakeLlm.requests.slice(before).some((r) => r.images?.length)).toBe(true);
  await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
  return bubble;
}

/**
 * The page's own device pixels (CDP's capture, an explicit clip at scale 1:
 * Playwright's `scale: 'device'` goes by what it was told the ratio is).
 * Transitions finished first (a spinner's never ends: left as it is).
 */
async function devicePixels(main) {
  await main.evaluate(() => {
    for (const a of document.getAnimations()) {
      try {
        a.finish();
      } catch {
        // an endless one
      }
    }
  });
  const [width, height] = await main.evaluate(() => [window.innerWidth, window.innerHeight]);
  const cdp = await main.context().newCDPSession(main);
  try {
    const { data } = await cdp.send('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width, height, scale: 1 } });
    return Buffer.from(data, 'base64');
  } finally {
    await cdp.detach().catch(() => {});
  }
}

/** A named screenshot at the device's own pixels: test-results/app/screenshots/chat-shots/<name>.png. */
async function named(main, testInfo, name) {
  const bytes = await devicePixels(main);
  fs.mkdirSync(OUT, { recursive: true });
  const file = path.join(OUT, `${name}.png`);
  fs.writeFileSync(file, bytes);
  await testInfo.attach(name, { path: file, contentType: 'image/png' });
}

/** An <img> loaded and decoded. */
async function loaded(img) {
  await expect.poll(() => img.evaluate((el) => el.complete && el.naturalWidth > 0), { message: 'the picture to load' }).toBe(true);
  await img.evaluate((el) => el.decode().catch(() => {}));
}

/** An <img>'s box on screen (CSS px), its natural size, its address and the ratio. */
function placement(img) {
  return img.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height, nw: el.naturalWidth, nh: el.naturalHeight, src: el.getAttribute('src'), dpr: window.devicePixelRatio, fit: getComputedStyle(el).objectFit };
  });
}

/** Drawn at its own pixels: the box on screen is the picture's size in device pixels. */
async function expectOneToOne(img) {
  await loaded(img);
  const at = await placement(img);
  expect(Math.abs(at.w * at.dpr - at.nw), `${at.w} CSS px × ${at.dpr} vs ${at.nw} px`).toBeLessThan(0.6);
  expect(Math.abs(at.h * at.dpr - at.nh), `${at.h} CSS px × ${at.dpr} vs ${at.nh} px`).toBeLessThan(0.6);
  return at;
}

/** The ring's colour and width (CSS px) from a frame's ::after. */
async function ringOf(frame) {
  const shadow = await frame.evaluate((el) => getComputedStyle(el, '::after').boxShadow);
  const colour = shadow.match(/rgba?\([^)]*\)/)?.[0] ?? '';
  const lengths = [...shadow.matchAll(/(-?[\d.]+)px/g)].map((m) => Number(m[1]));
  return { colour, width: lengths.at(-1), inset: shadow.includes('inset') };
}

/**
 * A frame's top edge in device pixels: the row of the ring and the one
 * under it (the fill, where a wide picture leaves the square's top empty).
 */
async function topEdge(main, frame) {
  const r = await frame.evaluate((el) => {
    const b = el.getBoundingClientRect();
    return { x: b.x + b.width / 2, y: b.y, d: window.devicePixelRatio };
  });
  const page = readPng(await devicePixels(main));
  const x = Math.round(r.x * r.d);
  const y = Math.round(r.y * r.d);
  return { ring: pixel(page, x, y), under: pixel(page, x, y + 1) };
}

/**
 * How `img` (showing all of `source`) is drawn: its device pixels against
 * the ideal area-averaged copy, the crops kept beside the screenshots.
 */
async function measure(main, img, source, name) {
  await loaded(img);
  const at = await placement(img);
  const dpr = at.dpr;
  // The part of the picture drawn, and where (an older build's object-fit
  // cover crops it; contain letterboxes it in its box).
  let [dx, dy, dw, dh] = [at.x, at.y, at.w, at.h];
  let [fx, fy, fw, fh] = [0, 0, 1, 1];
  if (at.fit === 'cover' || at.fit === 'contain') {
    const s = at.fit === 'cover' ? Math.max(at.w / at.nw, at.h / at.nh) : Math.min(at.w / at.nw, at.h / at.nh);
    const [sw, sh] = [at.nw * s, at.nh * s];
    if (at.fit === 'cover') [fx, fy, fw, fh] = [(sw - at.w) / 2 / sw, (sh - at.h) / 2 / sh, at.w / sw, at.h / sh];
    else [dx, dy, dw, dh] = [at.x + (at.w - sw) / 2, at.y + (at.h - sh) / 2, sw, sh];
  }
  const page = readPng(await devicePixels(main));
  const [px, py] = [Math.round(dx * dpr), Math.round(dy * dpr)];
  const drawn = crop(page, px, py, Math.round((dx + dw) * dpr) - px, Math.round((dy + dh) * dpr) - py);
  expect(drawn.width * drawn.height, 'something drawn').toBeGreaterThan(0);
  const ideal = areaAverage(source, fx * source.width, fy * source.height, fw * source.width, fh * source.height, drawn.width, drawn.height);
  const result = compare(drawn, ideal, Math.ceil(2 * dpr), Math.ceil(12 * dpr));
  fs.mkdirSync(OUT, { recursive: true });
  const stem = `${LABEL}-${name}-${Math.round(dpr * 100)}`;
  fs.writeFileSync(path.join(OUT, `${stem}-drawn-vs-ideal.png`), encode(sideBySide(enlarge(crop(drawn, 0, 0, 160, 120), 4), enlarge(crop(ideal, 0, 0, 160, 120), 4))));
  return { ...result, at: [dx * dpr, dy * dpr], drawn: [drawn.width, drawn.height], natural: [at.nw, at.nh], thumb: /[?&]w=/.test(at.src ?? '') };
}

/** The tests, at `scale` (1 or 1.5). */
export function shotTests(scale) {
  const pct = Math.round(scale * 100);
  const test = base.extend({
    fakeLlm: [
      async ({}, use) => {
        const fake = await startFakeLlm();
        await use(fake);
        await fake.close();
      },
      { scope: 'worker' },
    ],
    yapOptions: [
      async ({ fakeLlm }, use) => {
        await use({
          name: `chat-shots-${pct}`,
          // The scaling asked for whatever the monitor's (100 % too: a
          // developer's 150 % screen doesn't change the 100 % run).
          env: { WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `${WEBVIEW2_ARGS} --force-device-scale-factor=${scale}` },
          config: {
            ppApiKeys: { groq: TEST_KEY },
            llmScopes: {
              chat: { enabled: true, provider: 'groq', baseUrl: fakeLlm.base, model: 'qwen/qwen3.8-27b', apiKey: TEST_KEY, apiKeys: {}, prompt: '', disableThinking: false },
            },
            meetingReminder: 'never',
          },
        });
      },
      { scope: 'worker' },
    ],
  });

  /**
   * The run at the scaling asked for, or skipped: an elevated WebView2 (a CI
   * runner) ignores WEBVIEW2_* variables, so it stays at the monitor's.
   */
  async function atScale(main) {
    const dpr = await main.evaluate(() => window.devicePixelRatio);
    test.skip(dpr !== scale, `WebView2 is at ${dpr}x, not ${scale}x: an elevated run ignores WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`);
  }

  test(`at ${pct} %: the box's tile — a 120 px square, the whole picture at its device pixels, the ring over its edge, ✕ and size on hover`, async ({ yap, main }, testInfo) => {
    await atScale(main);
    for (const theme of ['light', 'dark']) {
      await setConfig(yap, main, { theme });
      await freshChat(main);
      await drop(main, yap, `wide-${theme}.png`, SOURCE, 1);
      const chip = attachments(main).locator('.shotchip').first();
      const img = chip.locator('img');
      await expect(attachments(main)).toContainText('1920 × 1080');
      // A 120 px square, whatever the picture's shape.
      const frame = await chip.boundingBox();
      expect(Math.abs(frame.width - 120)).toBeLessThan(0.5);
      expect(Math.abs(frame.height - 120)).toBeLessThan(0.5);
      // Rust's thumbnail at the tile's device pixels, the whole picture (16:9
      // in the square: 120 × 68 at 100 %, 180 × 101 at 150 %), drawn 1:1.
      const want = contain(1920, 1080, Math.round(120 * scale), Math.round(120 * scale));
      const at = await expectOneToOne(img);
      expect([at.nw, at.nh]).toEqual(want);
      expect(at.src).toContain(`?w=${want[0]}&h=${want[1]}`);
      // Kept beside the image, in thumbs/.
      const id = at.src.match(/img_[0-9a-f]{16}/)[0];
      expect(fs.existsSync(path.join(yap.dataDir, 'chat-images', 'thumbs', `${id}-${want[0]}x${want[1]}.png`))).toBe(true);
      // The ring: over the picture's edge (inset), the theme's colour, one
      // device pixel wide; on screen, the fill lifted by it on the top row
      // only (the "white tinge" in the dark).
      await main.mouse.move(1, 1);
      await expect.poll(async () => (await ringOf(chip)).colour).toBe(theme === 'dark' ? 'rgba(255, 255, 255, 0.2)' : 'rgba(35, 33, 27, 0.16)');
      const ring = await ringOf(chip);
      expect(ring.inset).toBe(true);
      expect(Math.abs(ring.width * scale - 1)).toBeLessThan(0.01);
      const edge = await topEdge(main, chip);
      expect(near(edge.ring, ringOver(theme), 3), `ring ${edge.ring} vs ${ringOver(theme)}`).toBe(true);
      expect(near(edge.under, LOOK[theme].fill, 2), `fill ${edge.under} vs ${LOOK[theme].fill}`).toBe(true);
      // ✕ and the size show on hover, out over the corner.
      const x = chip.getByRole('button', { name: 'Remove the screenshot' });
      const size = chip.locator('.shotsize');
      await expect(x).toHaveCSS('opacity', '0');
      await expect(size).toHaveCSS('opacity', '0');
      await named(main, testInfo, `box-${theme}-${pct}`);
      await chip.hover();
      await expect(x).toHaveCSS('opacity', '1');
      await expect(size).toHaveCSS('opacity', '1');
      await expect(size).toHaveText('1920 × 1080');
      const xBox = await x.boundingBox();
      expect(xBox.x + xBox.width).toBeGreaterThan(frame.x + frame.width);
      expect(xBox.y).toBeLessThan(frame.y);
      await named(main, testInfo, `box-${theme}-${pct}-hover`);
      // ✕ takes it out.
      await x.click();
      await expect(attachments(main)).toBeHidden();
    }
  });

  test(`at ${pct} %: a question's pictures — their own pixels when they fit, at most 480 × 320 (240 each of several), never under a tile`, async ({ yap, main, fakeLlm }, testInfo) => {
    await atScale(main);
    for (const theme of ['light', 'dark']) {
      await setConfig(yap, main, { theme });
      await freshChat(main);
      // A small snip alone: at its own pixels (the picture itself, no
      // thumbnail) — 200 × 100 device px, 133 CSS px wide at 150 % — in a
      // frame at least a tile tall, its words in the bubble under it.
      await drop(main, yap, 'small.png', SMALL, 1);
      const alone = await ask(main, fakeLlm, `This snip (${theme})`);
      const small = await expectOneToOne(alone.locator('.shots img'));
      expect([small.nw, small.nh]).toEqual([200, 100]);
      expect(small.src).not.toContain('?w=');
      const smallFrame = await alone.locator('.shots .shotframe').boundingBox();
      expect(Math.abs(smallFrame.height - 120)).toBeLessThan(0.6);
      expect(Math.abs(smallFrame.width - Math.max(120, 200 / scale))).toBeLessThan(0.6);
      await expect(alone.locator('.utext')).toHaveText(`This snip (${theme})`);
      // A whole screen alone: its own shape, at most 480 × 320 (less in a
      // narrow window), Rust's thumbnail at exactly its device pixels.
      await drop(main, yap, 'screen.png', SOURCE, 1);
      const one = await ask(main, fakeLlm, `This screen (${theme})`);
      const frame = await one.locator('.shots .shotframe').boundingBox();
      expect(frame.width).toBeLessThanOrEqual(480.5);
      expect(frame.height).toBeLessThanOrEqual(320.5);
      const screen = await expectOneToOne(one.locator('.shots img'));
      expect(screen.src).toContain('?w=');
      expect(Math.abs(screen.nw / screen.nh - 1920 / 1080)).toBeLessThan(0.02);
      await main.mouse.move(1, 1);
      await expect.poll(async () => (await ringOf(one.locator('.shots .shotframe'))).colour).toBe(theme === 'dark' ? 'rgba(255, 255, 255, 0.2)' : 'rgba(35, 33, 27, 0.16)');
      expect(Math.abs((await ringOf(one.locator('.shots .shotframe'))).width * scale - 1)).toBeLessThan(0.01);
      // Two in one question: side by side, 240 CSS px wide each at most, each
      // at its device pixels.
      await drop(main, yap, 'small.png', SMALL, 1);
      await drop(main, yap, 'screen.png', SOURCE, 2);
      const two = await ask(main, fakeLlm, `Compare these (${theme})`);
      const frames = two.locator('.shots .shotframe');
      await expect(frames).toHaveCount(2);
      for (const i of [0, 1]) {
        expect((await frames.nth(i).boundingBox()).width).toBeLessThanOrEqual(240.5);
        await expectOneToOne(frames.nth(i).locator('img'));
      }
      // Side by side when the thread has room (a narrow one wraps them),
      // lined up at the foot.
      const [a, b] = [await frames.nth(0).boundingBox(), await frames.nth(1).boundingBox()];
      if (a.x + a.width <= b.x + 0.5) expect(Math.abs(a.y + a.height - (b.y + b.height)), 'lined up at the foot').toBeLessThan(1);
      await named(main, testInfo, `sent-${theme}-${pct}`);
    }
  });

  test(`at ${pct} %: the right-click menu still opens, copies and saves; the viewer shows a picture 1:1 when it fits`, async ({ yap, main, fakeLlm }, testInfo) => {
    await atScale(main);
    await setConfig(yap, main, { theme: 'dark' });
    await freshChat(main);
    await drop(main, yap, 'small.png', SMALL, 1);
    // The tile's menu.
    const tile = attachments(main).locator('.shotchip .thumb').first();
    const id = (await attachments(main).locator('.shotchip img').first().getAttribute('src')).match(/img_[0-9a-f]{16}/)[0];
    const file = path.join(yap.dataDir, 'chat-images', `${id}.png`);
    await tile.click({ button: 'right' });
    await expect(main.getByRole('menuitem', { name: 'Open' })).toBeVisible();
    await expect(main.getByRole('menuitem', { name: 'Save image as…' })).toBeVisible();
    await main.getByRole('menuitem', { name: 'Copy image' }).click();
    await expect.poll(() => yap.invoke('e2e_snip_clipboard')).toBe(fs.readFileSync(file).toString('base64'));
    // Sent: its picture's menu → Open → the viewer, at one image pixel per device pixel.
    const bubble = await ask(main, fakeLlm, 'What colour is this?');
    await bubble.locator('.shots .shot').first().click({ button: 'right' });
    await main.getByRole('menuitem', { name: 'Open' }).click();
    const large = main.locator('.shotview img');
    const at = await expectOneToOne(large);
    expect([at.nw, at.nh]).toEqual([200, 100]);
    expect(at.src).not.toContain('?w=');
    // The viewer's own menu saves it.
    const saved = path.join(yap.dataDir, 'saved-snips', `viewer-${pct}.png`);
    fs.mkdirSync(path.dirname(saved), { recursive: true });
    await yap.invoke('e2e_snip_save_path', { path: saved });
    await large.click({ button: 'right' });
    await main.getByRole('menuitem', { name: 'Save image as…' }).click();
    await expect.poll(() => fs.existsSync(saved)).toBe(true);
    expect(fs.readFileSync(saved).equals(fs.readFileSync(file))).toBe(true);
    await named(main, testInfo, `open-small-dark-${pct}`);
    await main.keyboard.press('Escape');
    await expect(large).toBeHidden();
    // A whole screen opened: as big as the window lets it, Rust's thumbnail
    // at exactly those device pixels, its ring.
    await drop(main, yap, 'screen.png', SOURCE, 1);
    const screen = await ask(main, fakeLlm, 'And this screen?');
    await screen.locator('.shots .shot').first().click();
    const big = await expectOneToOne(large);
    expect(big.src).toContain('?w=');
    const win = await main.evaluate(() => [window.innerWidth, window.innerHeight]);
    expect(big.w).toBeLessThanOrEqual(win[0] - 63);
    expect(big.h).toBeLessThanOrEqual(win[1] - 63);
    await main.mouse.move(1, 1);
    await expect.poll(async () => (await ringOf(main.locator('.shotview .shotframe'))).colour).toBe('rgba(255, 255, 255, 0.2)');
    await named(main, testInfo, `open-dark-${pct}`);
    await main.keyboard.press('Escape');
    await setConfig(yap, main, { theme: 'light' });
    await screen.locator('.shots .shot').first().click();
    await loaded(large);
    await named(main, testInfo, `open-light-${pct}`);
    await main.keyboard.press('Escape');
    await expect(large).toBeHidden();
  });

  test(`at ${pct} %: measure — how crisply a screenshot is drawn in the box, in a question and opened large`, async ({ yap, main, fakeLlm }, testInfo) => {
    await atScale(main);
    await setConfig(yap, main, { theme: 'dark' });
    await freshChat(main);
    await drop(main, yap, 'editor.png', SOURCE, 1);
    const results = {};
    results.box = await measure(main, attachments(main).locator('.shotchip img').first(), SOURCE, 'box');
    await named(main, testInfo, `${LABEL}-box-${pct}`);
    const before = fakeLlm.requests.length;
    await box(main).press('Enter');
    const sent = main.locator('.cbubble.user .shots img').last();
    await expect(sent).toBeVisible();
    await expect.poll(() => fakeLlm.requests.slice(before).some((r) => r.images?.length)).toBe(true);
    await expect(main.locator('.cbubble.assistant').last()).toContainText('OK');
    results.sent = await measure(main, sent, SOURCE, 'sent');
    await named(main, testInfo, `${LABEL}-sent-${pct}`);
    await main.locator('.cbubble.user .shots .shot').last().click();
    const large = main.locator('.shotview img');
    results.open = await measure(main, large, SOURCE, 'open');
    await named(main, testInfo, `${LABEL}-open-${pct}`);
    await main.keyboard.press('Escape');
    await expect(large).toBeHidden();
    fs.mkdirSync(OUT, { recursive: true });
    fs.writeFileSync(path.join(OUT, `${LABEL}-metrics-${pct}.json`), JSON.stringify(results, null, 2));
    console.log(`${LABEL} at ${pct} %: ${JSON.stringify(results)}`);
    if (LABEL === 'after') {
      // Drawn as the ideal: Rust's thumbnail at exactly its device pixels.
      for (const [where, r] of Object.entries(results)) {
        expect(r.error, `${where}: mean difference from the ideal`).toBeLessThan(1);
        expect(Math.abs(r.sharpness - 1), `${where}: edge contrast against the ideal's`).toBeLessThan(0.03);
      }
    }
  });
}
