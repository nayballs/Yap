// A question's pictures (the Claude app's layout, 2026-11-03): + → Take
// photo / Choose photos, each made smaller on the phone (2,048 px at most, a
// JPEG) and shown as a thumbnail inside the composer card with ✕; sent
// before the question that takes them (`picture {n, size}`, then its 0x03
// frames, then `ask` / `stop` with `pictures`); four at most; Chat's line
// when its model can't read them (another model, their text instead) and
// the PC's `cant_read`; a PC from before has no +. Yap on the PC mocked.
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc,
  open,
  ready,
  ask,
  snapshot,
  pickerData,
  photo,
  jpegSize,
  choosePhotos,
  CAN_BEFORE_CHATS,
  CAN_BEFORE_WORDS,
} from './support/harness.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

/** Chat on Claude Opus 5.5, which reads images (the default mock's on-device Qwen doesn't). */
const reads = () => ({
  picker: pickerData({ model: 'opus' }),
  model: { id: 'anthropic|claude-opus-5-5', label: 'Claude Opus 5.5 · Anthropic', onThisPc: false },
});

test('+ → Choose photos: thumbnails in the card with ✕, made smaller before they go', async ({ page }) => {
  const pc = await mockPc(page, reads());
  await open(page);
  await ready(page, pc);
  // A big one and a small one.
  await choosePhotos(page, [await photo(page, { w: 3000, h: 2000, name: 'garden.png' }), await photo(page, { w: 800, h: 1200, hue: 20, name: 'pot.png' })]);
  await expect(page.getByTestId('plus-menu')).toBeHidden();
  await expect(page.getByTestId('shot')).toHaveCount(2);
  const sizes = (await snapshot(page)).attached;
  expect(sizes.map((s) => [s.w, s.h])).toEqual([
    [2048, 1365],
    [800, 1200],
  ]);
  for (const s of sizes) expect(s.bytes).toBeLessThan(3 * 1024 * 1024);
  // ✕ takes one out.
  await page.getByTestId('shot-remove').first().click();
  await expect(page.getByTestId('shot')).toHaveCount(1);
  // Nothing goes until the question does.
  expect(pc.of('picture')).toHaveLength(0);
  // No line: Chat's model reads images.
  await expect(page.getByTestId('vision')).toBeHidden();
});

test('a typed question with pictures: each sent in pieces, then ask with how many', async ({ page }) => {
  const pc = await mockPc(page, reads());
  await open(page);
  await ready(page, pc);
  await choosePhotos(page, [await photo(page, { w: 3000, h: 2000 }), await photo(page, { w: 640, h: 480, hue: 120 })]);
  await expect(page.getByTestId('shot')).toHaveCount(2);
  await page.getByTestId('input').fill('Which of these two is better for a shady corner?');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').length).toBe(1);
  expect(pc.of('ask')[0]).toEqual({ type: 'ask', text: 'Which of these two is better for a shady corner?', pictures: 2 });
  // Each picture announced, then its bytes whole, in pieces in order, before the question.
  expect(pc.of('picture').map((p) => [p.n, p.size > 0])).toEqual([
    [0, true],
    [1, true],
  ]);
  const order = pc.log.map((l) => (l.text ? l.text.type : l.picture ? 'piece' : 'audio')).filter((t) => t !== 'ping');
  expect(order.lastIndexOf('piece')).toBeLessThan(order.indexOf('ask'));
  for (const p of pc.pictures) {
    expect(p.bytes.length).toBe(p.size);
    expect(p.pieces).toBe(Math.ceil(p.size / 32000));
  }
  expect(jpegSize(pc.pictures[0].bytes)).toEqual({ w: 2048, h: 1365 });
  expect(jpegSize(pc.pictures[1].bytes)).toEqual({ w: 640, h: 480 });
  // The composer is empty again; the question shows its pictures.
  await expect(page.getByTestId('shot')).toHaveCount(0);
  await expect(page.getByTestId('turn').last().getByTestId('turn-pictures').locator('img')).toHaveCount(2);
  expect((await snapshot(page)).turns.at(-1)).toMatchObject({ pictures: 2, typed: true });
});

test('a spoken question sent at Done (hands-free, a PC from before words) takes the pictures too: sent before stop', async ({ page }) => {
  // (Talk's words for the box leave them in the composer for Send: words.spec.js.)
  const pc = await mockPc(page, { ...reads(), can: CAN_BEFORE_WORDS });
  await open(page);
  await ready(page, pc);
  await choosePhotos(page, [await photo(page, { w: 900, h: 900 })]);
  await expect(page.getByTestId('shot')).toHaveCount(1);
  await ask(page, pc, 300);
  expect(pc.of('stop').at(-1)).toEqual({ type: 'stop', pictures: 1 });
  expect(pc.pictures).toHaveLength(1);
  const order = pc.log.map((l) => (l.text ? l.text.type : l.picture ? 'piece' : 'audio')).filter((t) => t !== 'ping');
  expect(order.lastIndexOf('piece')).toBeLessThan(order.lastIndexOf('stop'));
  expect(order.indexOf('piece')).toBeGreaterThan(order.lastIndexOf('audio'));
  await expect(page.getByTestId('shot')).toHaveCount(0);
});

test('four pictures at most', async ({ page }) => {
  const pc = await mockPc(page, reads());
  await open(page);
  await ready(page, pc);
  const files = [];
  for (let i = 0; i < 5; i++) files.push(await photo(page, { w: 320, h: 240, hue: i * 60, name: `p${i}.png` }));
  await choosePhotos(page, files);
  await expect(page.getByTestId('shot')).toHaveCount(4);
  await expect(page.getByTestId('note')).toHaveText('A question can carry 4 pictures at most: the first 4 were added.');
  await choosePhotos(page, [files[4]]);
  await expect(page.getByTestId('note')).toHaveText('A question can carry 4 pictures at most.');
  await expect(page.getByTestId('shot')).toHaveCount(4);
});

test("Chat's model can't read images: its line, the switch, or their text instead", async ({ page }) => {
  // The default mock: Chat on the on-device Qwen, which can't; the PC offers Groq's Qwen3.8.
  const data = pickerData();
  data.status.imagesSwitch = { provider: 'groq', model: 'qwen/qwen3.8-27b', name: 'Qwen3.8 27B' };
  const pc = await mockPc(page, { picker: data });
  await open(page);
  await ready(page, pc);
  await choosePhotos(page, [await photo(page, { w: 400, h: 300 })]);
  const line = page.getByTestId('vision');
  await expect(line).toBeVisible();
  await expect(line).toContainText("Qwen3.5 4B can't read images.");
  await expect(page.getByTestId('vision-switch')).toHaveText('Use Qwen3.8 27B on Groq');
  await expect(page.getByTestId('vision-as-text')).toBeDisabled();
  // Send doesn't send the pictures past it: the line has the ways on.
  await page.getByTestId('input').fill('What does this sign say?');
  await page.getByTestId('input').blur();
  await page.waitForTimeout(250);
  await page.screenshot({ path: 'test-results/link/screenshots/vision-390x844-light.png' });
  await page.getByTestId('input').press('Enter');
  await page.waitForTimeout(200);
  expect(pc.of('ask')).toHaveLength(0);
  // Their text instead: read on the PC, asked with asText.
  await page.getByTestId('vision-as-text').click();
  await expect.poll(() => pc.of('ask').length).toBe(1);
  expect(pc.of('ask')[0]).toEqual({ type: 'ask', text: 'What does this sign say?', pictures: 1, asText: true });
  await expect(line).toBeHidden();

  // Again, with the switch: set_model first, then the question, in that order.
  pc.send({ type: 'answer', text: 'It says "Mind the step".', final: true, id: 'chat:3:1' });
  pc.send({ type: 'state', phase: 'idle' });
  await choosePhotos(page, [await photo(page, { w: 400, h: 300, hue: 300 })]);
  await page.getByTestId('input').fill('And this one?');
  await page.getByTestId('vision-switch').click();
  await expect.poll(() => pc.of('ask').length).toBe(2);
  const types = pc.types().filter((t) => t !== 'ping');
  expect(types.slice(-3)).toEqual(['set_model', 'picture', 'ask']);
  expect(pc.of('set_model').at(-1)).toEqual({ type: 'set_model', provider: 'groq', model: 'qwen/qwen3.8-27b', baseUrl: null, file: null });
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'And this one?', pictures: 1 });
});

test("the PC's cant_read: the question back in the composer with its pictures, and the line", async ({ page }) => {
  const pc = await mockPc(page, reads());
  await open(page);
  await ready(page, pc);
  await choosePhotos(page, [await photo(page, { w: 500, h: 500 })]);
  await page.getByTestId('input').fill('Is this mushroom safe?');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').length).toBe(1);
  await expect(page.getByTestId('turn')).toHaveCount(1);
  // The model's server turned the picture down.
  pc.send({ type: 'heard', text: 'Is this mushroom safe?' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'cant_read', message: "The model couldn't read the screenshot (claude-opus-5-5)" });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('vision')).toContainText("Claude Opus 5.5 couldn't read the picture.");
  await expect(page.getByTestId('turn')).toHaveCount(0);
  await expect(page.getByTestId('input')).toHaveValue('Is this mushroom safe?');
  await expect(page.getByTestId('shot')).toHaveCount(1);
  // Send without them.
  await page.getByTestId('vision-without').click();
  await expect.poll(() => pc.of('ask').length).toBe(2);
  expect(pc.of('ask')[1]).toEqual({ type: 'ask', text: 'Is this mushroom safe?' });
  await expect(page.getByTestId('shot')).toHaveCount(0);
});

test('a PC from before pictures: no +', async ({ page }) => {
  const pc = await mockPc(page, { can: CAN_BEFORE_CHATS });
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('plus')).toBeHidden();
  const old = await mockPc(page, { old: true });
  await open(page);
  await ready(page, old);
  await expect(page.getByTestId('plus')).toBeHidden();
});
