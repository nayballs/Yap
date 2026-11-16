// Maths, diagrams and pictures on the Yap bar (16 Nov 2026; after
// markdown-visuals.spec.js, which checks them in Chat's answers, notes and
// documents): core/markdown.js marks them, ui/markdownVisuals.js draws them,
// KaTeX and mermaid in chunks of their own loaded the first time one shows.
// - The Voice Assistant's answer card (bar/BarCard.svelte, style "answer"):
//   inline and display maths drawn (a price stays text), a ```mermaid
//   diagram drawn in the bar's colours (its own Light / Dark, not the
//   app's), a picture as "Show image" — nothing fetched until it's clicked —
//   and the card's region growing with the picture (the bar is click-through
//   outside its regions). No shadows or backdrop-filter on what's drawn: the
//   window is transparent.
// - A notice whose body is a model's words (`Card.markdown`: an agent's
//   reply, a routine's answer): its maths and bold drawn.
// - The reading card (bar/ReadingCard.svelte, bar/readingPieces.js): an
//   answer's maths read aloud is drawn by KaTeX in the sentence, lit as one
//   while any of its words is said.
//
// The cards are the debug-only demo cards (`bar_simulate`); the reading is
// `read_aloud_text` (test mode plays nothing: the voice is a stand-in). The
// web is a local fake (support/fake-web.js). Fictional content only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, setConfig } from './support/fixtures.js';
import { startFakeWeb, FORT_JOY_MAP } from './support/fake-web.js';
import { SCREENSHOTS } from './support/yap.js';

const test = base.extend({
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeWeb }, use) => {
      await use({
        name: 'markdown-everywhere',
        env: { ...fakeWeb.env, YAP_E2E_NET_ALIASES: `${new URL(fakeWeb.base).host}=wiki.example.com` },
      });
    },
    { scope: 'worker' },
  ],
});

const pictureAsks = (fakeWeb) => fakeWeb.requests.filter((r) => r.service === 'standin' && r.path === new URL(FORT_JOY_MAP).pathname).length;

/** One element of the bar as a picture, into this spec's folder. */
async function elementShot(locator, name) {
  const file = path.join(SCREENSHOTS, 'markdown-everywhere', `${name}.png`);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  await locator.screenshot({ path: file, animations: 'disabled' });
}

/** The drawn diagram's own SVG (in its shadow root): its labels and its node fill. */
const diagramIn = (box) =>
  box.evaluate((el) => {
    const root = el.querySelector('.md-diagram-view')?.shadowRoot;
    const svg = root?.querySelector('svg');
    const rect = root?.querySelector('.node rect, .node polygon, .node path');
    return svg ? { labels: svg.textContent, fill: rect ? getComputedStyle(rect).fill : null } : null;
  });

/** What's drawn in `card` never has a shadow or a backdrop-filter (the bar's window is transparent). */
const anyShadow = (card) =>
  card.evaluate((el) =>
    [...el.querySelectorAll('.md-imgbox, .md-diagram, .md-diagram-view, .md-math, img.md-img')]
      .map((x) => getComputedStyle(x))
      .some((s) => s.boxShadow !== 'none' || (s.backdropFilter && s.backdropFilter !== 'none'))
  );

async function barTheme(yap, main, which) {
  await setConfig(yap, main, { barTheme: which });
  // ("app", Match app, shows as the app's own theme.)
  if (which !== 'app') await expect.poll(() => yap.overlay.evaluate(() => document.documentElement.dataset.barTheme)).toBe(which);
}

test.afterEach(async ({ yap }) => {
  await yap.invoke('read_aloud_action', { action: 'stop' }).catch(() => {});
  await yap.invoke('tts_stop').catch(() => {});
  await yap.invoke('bar_card_action', { id: 'demo', action: 'close' }).catch(() => {});
});

test("the Voice Assistant's answer card draws its maths, diagram and (when asked) its picture, in the bar's colours", async ({ yap, main, fakeWeb, shot }) => {
  // The app light, the bar on its own: Light, then Dark.
  await setConfig(yap, main, { theme: 'light' });
  await barTheme(yap, main, 'light');
  const bar = yap.overlay;
  await yap.invoke('bar_simulate', { card: 'answer' });
  const card = bar.locator('.card').filter({ hasText: 'area of a circle' });
  await expect(card).toBeVisible();
  const body = card.locator('.body.answer');

  // Maths: two inline formulas and one on lines of its own, drawn by KaTeX; the price is text.
  await expect(body.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(3);
  await expect(body.locator('.md-math-display .katex-display')).toHaveCount(1);
  await expect(body.locator('p').filter({ hasText: 'a $5 ruler is plenty' })).toHaveCount(1);
  expect(await body.locator('.md-math .katex').first().evaluate((el) => getComputedStyle(el).fontFamily)).toContain('KaTeX_Main');

  // The diagram: drawn in a shadow root, its code hidden.
  const box = body.locator('.md-diagram');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  const light = await diagramIn(box);
  expect(light.labels).toContain('Hotkey');
  await expect(box.locator('pre.md-code')).toBeHidden();

  expect(await anyShadow(card)).toBe(false);
  await bar.waitForTimeout(200);
  await shot(bar, '01-answer-card-light');
  await elementShot(card, '01-answer-card-light-card');

  // The picture: a box naming its site, in the bar's colours; nothing fetched until it's clicked.
  const pic = body.getByRole('button', { name: /^Show image: Fort Joy map, from wiki\.example\.com/ });
  await pic.scrollIntoViewIfNeeded();
  await expect(pic).toBeVisible();
  const before = pictureAsks(fakeWeb);
  await bar.waitForTimeout(400);
  expect(pictureAsks(fakeWeb)).toBe(before);
  await elementShot(card, '02-answer-card-picture-box-light');

  await pic.click();
  const img = body.locator('img.md-img');
  await expect(img).toBeVisible();
  await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
  expect(pictureAsks(fakeWeb)).toBe(before + 1);
  // The card's region follows it (Rust takes clicks only over its regions).
  await expect
    .poll(async () => {
      const r = await card.evaluate((el) => el.getBoundingClientRect().height);
      const region = (await yap.invoke('bar_debug')).regions.find((x) => x.id === 'card:demo');
      return region ? Math.abs(region.h - r) < 2 : false;
    })
    .toBe(true);
  await img.scrollIntoViewIfNeeded();
  await elementShot(card, '02-answer-card-picture-light');

  // The bar Dark while the app stays light: the diagram drawn again in the
  // bar's dark colours, everything on the card's own.
  await barTheme(yap, main, 'dark');
  await expect.poll(async () => (await diagramIn(box))?.fill).not.toBe(light.fill);
  expect(await main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('light');
  await body.evaluate((el) => (el.scrollTop = 0));
  await bar.waitForTimeout(200);
  await shot(bar, '03-answer-card-dark');
  await elementShot(card, '03-answer-card-dark-card');
  await img.scrollIntoViewIfNeeded();
  await elementShot(card, '03-answer-card-picture-dark');
  await barTheme(yap, main, 'app');
});

test("a notice with a model's words draws its maths; Yap's own notices stay text", async ({ yap, main, shot }) => {
  await setConfig(yap, main, { theme: 'light' });
  await barTheme(yap, main, 'light');
  const bar = yap.overlay;
  await yap.invoke('bar_simulate', { card: 'markdown' });
  const card = bar.locator('.card').filter({ hasText: 'Claude replied: maths-notes' });
  await expect(card).toBeVisible();
  const body = card.getByTestId('bar-card-markdown');
  await expect(body.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(2);
  await expect(body.locator('strong')).toHaveText('notes.md');
  await expect(body.locator('.katex .mfrac')).toHaveCount(1);
  await bar.waitForTimeout(200);
  await shot(bar, '04-notice-maths-light');
  await barTheme(yap, main, 'dark');
  await bar.waitForTimeout(200);
  await shot(bar, '05-notice-maths-dark');

  // An update card (Yap's own words): its body is text, never rendered.
  await yap.invoke('bar_simulate', { card: 'update' });
  const update = bar.locator('.card').filter({ hasText: 'Yap 0.2.0 is ready' });
  await expect(update).toBeVisible();
  await expect(update.getByTestId('bar-card-markdown')).toHaveCount(0);
  await expect(update.locator('.body')).toHaveText('Restart Yap to finish updating. It only takes a few seconds.');
  await yap.invoke('bar_card_action', { id: 'demo', action: '' }).catch(() => {});
  await barTheme(yap, main, 'app');
});

test('Read aloud draws an answer\'s maths on the reading card, lit as one while it\'s said', async ({ yap, main, shot }) => {
  await setConfig(yap, main, { ttsRate: 1, theme: 'light', readingFont: 'default', readingSize: 'default' });
  await barTheme(yap, main, 'light');
  await yap.invoke('read_aloud_text', { text: 'The area of a circle is $A = \\pi r^2$ for any radius, as the notes say.', markdown: true, source: 'chat' });
  const c = yap.overlay.locator('[data-region="reading"].card');
  await expect(c).toBeVisible();
  const text = c.locator('.text');
  await expect(text).toContainText('The area of a circle is');
  await expect(text).toContainText('for any radius');

  // The formula drawn in the sentence, as one piece holding its words.
  const formula = text.locator('.w.md-math[data-drawn="true"]');
  await expect(formula).toHaveCount(1);
  await expect(formula.locator('.katex')).toHaveCount(1);
  await expect(formula.locator('.katex annotation')).toHaveText('A = \\pi r^2');
  const words = (await formula.getAttribute('data-w')).split(' ').map(Number);
  expect(words.length).toBeGreaterThanOrEqual(2);
  // Lit as a whole while one of its words is said (the karaoke goes on through it): held there.
  await expect.poll(() => text.locator('.w.md-math.now').count(), { timeout: 10_000, intervals: [30] }).toBe(1);
  await yap.invoke('read_aloud_action', { action: 'pause' });
  await expect(c).toHaveAttribute('data-phase', 'paused');
  expect(words).toContain((await yap.invoke('read_aloud_state')).word);
  await elementShot(c, '06-reading-maths-light');
  await shot(yap.overlay, '06-bar-reading-maths-light');
  await barTheme(yap, main, 'dark');
  await yap.overlay.waitForTimeout(200);
  await elementShot(c, '07-reading-maths-dark');

  // Maths on lines of its own: drawn as display maths, in Dark.
  await yap.invoke('read_aloud_action', { action: 'stop' });
  await expect(c).toHaveCount(0);
  await yap.invoke('read_aloud_text', { text: 'The sum:\n\n$$\n\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}\n$$', markdown: true, source: 'chat' });
  await yap.invoke('read_aloud_action', { action: 'pause' }).catch(() => {});
  await expect(c).toBeVisible();
  await expect.poll(async () => (await yap.invoke('read_aloud_state')).count).toBe(2);
  await yap.invoke('read_aloud_action', { action: 'next' });
  await yap.invoke('read_aloud_action', { action: 'pause' });
  const display = text.locator('.w.md-math.md-math-display[data-drawn="true"]');
  await expect(display).toHaveCount(1, { timeout: 10_000 });
  await expect(display.locator('.katex .mfrac')).toHaveCount(1);
  await yap.overlay.waitForTimeout(200);
  await elementShot(c, '08-reading-display-maths-dark');

  // A price in a sentence stays words.
  await yap.invoke('read_aloud_action', { action: 'stop' });
  await expect(c).toHaveCount(0);
  await yap.invoke('read_aloud_text', { text: 'It costs $5 and $10 at most.', markdown: true, source: 'chat' });
  await expect(c).toBeVisible();
  await expect(text).toHaveText('It costs $5 and $10 at most.');
  await expect(text.locator('.md-math')).toHaveCount(0);
  await barTheme(yap, main, 'app');
});
