// An update's notes draw their maths, diagrams and pictures (16 Nov 2026), as
// Chat's answers do (ui/markdownVisuals.js on core/markdown.js's render):
// - the update toast's What's new, unfolded (ui/ToastHost.svelte), on the
//   toast's own dark colours whatever the app's theme;
// - Settings → About's "What's new in …" (settings/AboutPage.svelte);
// - Help → What's new (views/help/WhatsNew.svelte).
// A picture in them is "Show image": nothing fetched until it's clicked.
//
// A local feed announces the next patch version (support/update-feed.js), the
// first automatic check a few seconds after launch, as updates-auto.spec.js;
// the web is a local fake (support/fake-web.js). Fictional content only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, setConfig, openSettings, closeSettings } from './support/fixtures.js';
import { startUpdateFeed } from './support/update-feed.js';
import { startFakeWeb, FORT_JOY_MAP } from './support/fake-web.js';
import { APP, RUNS } from './support/yap.js';

const CURRENT = fs
  .readFileSync(path.join(APP, 'src-tauri', 'Cargo.toml'), 'utf8')
  .match(/^version\s*=\s*"([^"]+)"/m)[1];
const [major, minor, patch] = CURRENT.split('-')[0].split('.').map(Number);
const VERSION = `${major}.${minor}.${patch + 1}`;

const NOTES = [
  '## Faster dictation',
  '',
  '- A dictation of $n$ words now takes about $t = 0.1n$ seconds.',
  '- The cleanup costs $5 and $10 in nothing at all.',
  '',
  '$$',
  '\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}',
  '$$',
  '',
  '```mermaid',
  'flowchart LR',
  '  A[Hotkey] --> B[Record] --> C[Paste]',
  '```',
  '',
  `![Fort Joy map](${FORT_JOY_MAP})`,
].join('\n');

const test = base.extend({
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  updateFeed: [
    async ({}, use, workerInfo) => {
      const feed = await startUpdateFeed(path.join(RUNS, `update-feed-visuals-w${workerInfo.workerIndex}`), { version: VERSION, notes: NOTES });
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeWeb, updateFeed }, use) => {
      await use({
        name: 'update-notes-visuals',
        config: { updateChecksEnabled: true },
        env: {
          ...fakeWeb.env,
          YAP_E2E_NET_ALIASES: `${new URL(fakeWeb.base).host}=wiki.example.com`,
          YAP_UPDATE_TEST_ENDPOINT: updateFeed.endpoint,
          YAP_UPDATE_TEST_PUBKEY: updateFeed.pubkey,
          YAP_UPDATE_TEST_PORTABLE: '0',
          YAP_UPDATE_TEST_METERED: '0',
          // The first automatic check, a few seconds after launch.
          YAP_UPDATE_TEST_FIRST_CHECK: '3',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const pictureAsks = (fakeWeb) => fakeWeb.requests.filter((r) => r.service === 'standin' && r.path === new URL(FORT_JOY_MAP).pathname).length;

/** The drawn diagram's node fill (in its shadow root). */
const diagramFill = (box) =>
  box.evaluate((el) => {
    const rect = el.querySelector('.md-diagram-view')?.shadowRoot?.querySelector('.node rect, .node polygon, .node path');
    return rect ? getComputedStyle(rect).fill : null;
  });

/** Maths, the diagram and the picture's box drawn in `notes`; the price as text. */
async function expectDrawn(notes) {
  await expect(notes.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(3);
  await expect(notes.locator('.md-math-display .katex-display')).toHaveCount(1);
  await expect(notes).toContainText('costs $5 and $10 in nothing');
  const box = notes.locator('.md-diagram');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  await expect(box.locator('pre.md-code')).toBeHidden();
  await expect(notes.getByRole('button', { name: /^Show image: Fort Joy map, from wiki\.example\.com/ })).toHaveCount(1);
  return box;
}

test("the update toast's What's new draws its maths, diagram and picture box on the toast's dark card", async ({ yap, main, fakeWeb, shot }) => {
  test.setTimeout(180_000);
  await setConfig(yap, main, { theme: 'light' });
  // Yap's window on screen: the announcement is a toast (as updates-auto.spec.js).
  await yap.invoke('open_settings');
  // (Found, downloaded and checked first: slower on a busy PC.)
  await expect.poll(async () => (await yap.invoke('update_status')).status, { timeout: 90_000 }).toBe('ready');
  const ready = main.getByRole('status').filter({ hasText: `Yap ${VERSION} is ready` });
  await expect(ready).toBeVisible({ timeout: 30_000 });
  await ready.getByRole('button', { name: "What's new" }).click();
  const notes = ready.locator('.notes');
  const box = await expectDrawn(notes);
  // On the dark card in the light app: its diagram in dark colours, its picture box on the card's.
  const fill = await diagramFill(box);
  expect(fill).not.toBeNull();
  const boxBg = await notes.locator('.md-imgbox').evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(boxBg).toBe('rgba(255, 255, 255, 0.06)');
  const before = pictureAsks(fakeWeb);
  await main.waitForTimeout(300);
  expect(pictureAsks(fakeWeb)).toBe(before);
  await shot(main, '01-toast-whats-new-light-app');

  // The app dark: the toast's diagram stays as it was (the card was dark already).
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await main.waitForTimeout(300);
  expect(await diagramFill(box)).toBe(fill);
  await shot(main, '02-toast-whats-new-dark-app');
  await setConfig(yap, main, { theme: 'light' });
});

test("Settings → About and Help → What's new draw the notes, and a picture comes only when clicked", async ({ yap, main, fakeWeb, shot }) => {
  test.setTimeout(180_000);
  await setConfig(yap, main, { theme: 'light' });
  await yap.invoke('open_settings');
  await expect.poll(async () => (await yap.invoke('update_status')).status, { timeout: 90_000 }).toBe('ready');
  // (The toast from the first test out of the way.)
  for (const later of await main.getByRole('status').getByRole('button', { name: 'Later' }).all()) await later.click().catch(() => {});

  const dialog = await openSettings(main, 'About');
  const group = dialog.getByRole('group', { name: 'Updates' });
  await expect(group.getByText(`What’s new in ${VERSION}`)).toBeVisible();
  const notes = group.locator('.upd-notes-b');
  await expectDrawn(notes);
  await shot(main, '03-about-notes-light');
  // The picture: fetched once it's clicked, then shown.
  const before = pictureAsks(fakeWeb);
  await notes.getByRole('button', { name: /^Show image: Fort Joy map/ }).click();
  const img = notes.locator('img.md-img');
  await expect(img).toBeVisible();
  await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
  expect(pictureAsks(fakeWeb)).toBe(before + 1);
  await img.scrollIntoViewIfNeeded();
  await shot(main, '04-about-notes-picture-light');
  await closeSettings(main);

  // Help → What's new: the same notes in the dialog, light then dark.
  await main.getByRole('button', { name: 'Help', exact: true }).click();
  await main.getByRole('menu', { name: 'Help' }).getByRole('menuitem', { name: "What's new" }).click();
  const whatsNew = main.getByRole('dialog', { name: "What's new" });
  await expect(whatsNew).toBeVisible();
  const box = await expectDrawn(whatsNew.locator('.notes'));
  const light = await diagramFill(box);
  await shot(main, '05-help-whats-new-light');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => diagramFill(box)).not.toBe(light);
  await main.waitForTimeout(200);
  await shot(main, '06-help-whats-new-dark');
  await whatsNew.getByRole('button', { name: 'Close' }).click();
  await setConfig(yap, main, { theme: 'light' });
});
