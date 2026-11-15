// Maths, diagrams, diffs, headings and pictures in rendered markdown (15 Nov
// 2026, after the Claude app's answers): core/markdown.js marks them,
// ui/markdownVisuals.js draws them — KaTeX and mermaid in chunks of their
// own, loaded the first time one shows.
// - A Chat answer: inline and display maths drawn (a price stays text),
//   a ```mermaid diagram drawn in a shadow root with `</>` to its code and
//   back, a ```diff as red and green lines, h1–h4 at four sizes, and a
//   picture as "Show image" — nothing fetched until it's clicked, then the
//   picture from the (fake) web, through Rust (`markdown_picture`).
// - The theme changes: the diagram is drawn again in its colours.
// - KaTeX's sheet and fonts load from its chunk; a diagram with maths in a
//   node and on an edge still draws (that maths left blank), light and dark.
// - A note's summary (the Notes view): the same maths and diagram.
//
// The conversation and the note are seeded; nothing leaves the PC: the web is
// a local fake (support/fake-web.js, through the debug-only YAP_WEB_STANDIN).
// Fictional content only.
import { test as base, expect, expectStore, setConfig, openView } from './support/fixtures.js';
import { startFakeWeb, FORT_JOY_MAP } from './support/fake-web.js';

const NOW = Math.floor(Date.now() / 1000);

const ANSWER = [
  '# Dictation, in maths and pictures',
  '',
  '## Timing',
  '',
  'A dictation of $n$ words takes about $t = 0.12n + 0.4$ seconds, which costs $5 and $10 in nothing at all.',
  '',
  'The area under the curve:',
  '$$',
  '\\int_0^1 x^2\\,dx = \\frac{1}{3}',
  '$$',
  '',
  '### The flow',
  '',
  '```mermaid',
  'flowchart LR',
  '  A[Hotkey] --> B[Record]',
  '  B --> C[Transcribe]',
  '  C --> D{Cleanup on?}',
  '  D -- yes --> E[AI cleanup]',
  '  D -- no --> F[Paste]',
  '  E --> F',
  '```',
  '',
  '#### The change',
  '',
  '```diff',
  '@@ -1,2 +1,2 @@',
  '- const old = "removed line";',
  '+ const neu = "added line";',
  '  const same = "unchanged";',
  '```',
  '',
  `And the map: ![Fort Joy map](${FORT_JOY_MAP})`,
].join('\n');

// Maths beside a diagram with maths in its labels (mermaid draws `$$…$$` with
// KaTeX too: the one copy, vite.config.js `resolve.dedupe`).
const MATHS_ANSWER = [
  'The roots, inline: $x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}$, for any $a \\neq 0$.',
  '',
  '$$',
  '\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}',
  '$$',
  '',
  '```mermaid',
  'flowchart LR',
  '  A["$$x^2$$"] -->|"$$\\sqrt{x}$$"| B[Root]',
  '```',
].join('\n');

const CHATS = {
  conversations: [
    {
      id: 12,
      title: 'Maths in a diagram',
      createdTs: NOW - 900,
      updatedTs: NOW - 890,
      messages: [
        { role: 'user', text: 'Show me the roots and a diagram with maths in it.', ts: NOW - 900 },
        { role: 'assistant', text: MATHS_ANSWER, ts: NOW - 890 },
      ],
    },
    {
      id: 11,
      title: 'Maths and diagrams',
      createdTs: NOW - 600,
      updatedTs: NOW - 590,
      messages: [
        { role: 'user', text: 'Show me the dictation flow with maths and a diagram.', ts: NOW - 600 },
        { role: 'assistant', text: ANSWER, ts: NOW - 590 },
      ],
    },
  ],
};

// My thoughts: the page's editor (components/richtext/richVisuals.js) draws them too.
const THOUGHTS = ['Energy: $E = mc^2$, at $5 a slide.', '', '```mermaid', 'flowchart LR', '  Draft --> Check --> Ship', '```', '', 'Then the review.'].join('\n');

const NOTES = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 41,
      title: 'Physics review',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 3600,
      updatedTs: NOW - 3600,
      content: THOUGHTS,
      enhancedContent: ['## Summary', '', 'We agreed on $E = mc^2$ for the slide.', '', '```mermaid', 'flowchart LR', '  Draft --> Review --> Publish', '```'].join('\n'),
      transcript: [{ source: 'you', text: 'Energy is mass times the speed of light squared.', ts: NOW - 3590 }],
    },
  ],
};

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
        name: 'markdown-visuals',
        env: { ...fakeWeb.env, YAP_E2E_NET_ALIASES: `${new URL(fakeWeb.base).host}=wiki.example.com` },
        data: { 'chats.json': CHATS, 'notes.json': NOTES },
      });
    },
    { scope: 'worker' },
  ],
});

const answer = (main) => main.locator('.cbubble.assistant').last();
const pictureAsked = (fakeWeb) => fakeWeb.requests.some((r) => r.service === 'standin' && r.path === new URL(FORT_JOY_MAP).pathname);

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** The drawn diagram's own SVG (in its shadow root): its labels and its node fill. */
const diagramIn = (box) =>
  box.evaluate((el) => {
    const root = el.querySelector('.md-diagram-view')?.shadowRoot;
    const svg = root?.querySelector('svg');
    const rect = root?.querySelector('.node rect, .node polygon, .node path');
    return svg ? { labels: svg.textContent, fill: rect ? getComputedStyle(rect).fill : null, width: svg.getBoundingClientRect().width } : null;
  });

test('an answer draws its maths, diagram, diff, headings and (when asked) its picture', async ({ yap, main, fakeWeb, shot }) => {
  await theme(yap, main, 'light');
  await openView(main, 'Chat');
  await main.locator('.conv').filter({ hasText: 'Maths and diagrams' }).click();
  const a = answer(main);
  await expect(a).toContainText('Dictation, in maths and pictures');

  // Maths: inline and display, drawn by KaTeX; the prices stay text.
  await expect(a.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(3);
  await expect(a.locator('.md-math-display .katex-display')).toHaveCount(1);
  await expect(a.locator('p').filter({ hasText: 'costs $5 and $10 in nothing' })).toHaveCount(1);
  // (Its TeX kept for a copy.)
  await expect(a.locator('.md-math-display annotation')).toHaveText('\\int_0^1 x^2\\,dx = \\frac{1}{3}');

  // The diagram: drawn (in a shadow root), its code hidden; `</>` switches.
  const box = a.locator('.md-diagram');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  const light = await diagramIn(box);
  expect(light.labels).toContain('Hotkey');
  expect(light.labels).toContain('AI cleanup');
  expect(light.width).toBeGreaterThan(200);
  await expect(box.locator('pre.md-code')).toBeHidden();
  const sw = box.getByRole('button', { name: 'Show the code' });
  await expect(sw).toHaveAttribute('aria-pressed', 'false');
  await shot(main, '01-answer-light');
  await sw.click();
  await expect(box.locator('pre.md-code')).toBeVisible();
  await expect(box.locator('.md-diagram-view')).toBeHidden();
  await expect(box.getByRole('button', { name: 'Show the diagram' })).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '02-diagram-code');
  await box.getByRole('button', { name: 'Show the diagram' }).click();
  await expect(box.locator('.md-diagram-view')).toBeVisible();

  // The diff: red and green lines across the block, its text as written.
  const tint = (sel) => a.locator(sel).first().evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await tint('.md-dl.md-add')).not.toBe('rgba(0, 0, 0, 0)');
  expect(await tint('.md-dl.md-del')).not.toBe('rgba(0, 0, 0, 0)');
  expect(await tint('.md-dl.md-add')).not.toBe(await tint('.md-dl.md-del'));
  await expect(a.locator('pre.md-diff code')).toHaveText('@@ -1,2 +1,2 @@\n- const old = "removed line";\n+ const neu = "added line";\n  const same = "unchanged";\n');
  // One line each (no blank line between them).
  const heights = await a.locator('.md-dl').evaluateAll((els) => els.map((el) => el.getBoundingClientRect().height));
  expect(Math.max(...heights)).toBeLessThan(2 * Math.min(...heights));

  // Headings: # → ####, each clearly smaller than the one before.
  const sizes = await a.locator('h2, h3, h4, h5').evaluateAll((els) => els.map((el) => parseFloat(getComputedStyle(el).fontSize)));
  expect(sizes).toHaveLength(4);
  for (let i = 1; i < 4; i++) expect(sizes[i]).toBeLessThan(sizes[i - 1] - 0.9);

  // The picture: a box naming its site, nothing fetched until it's clicked.
  const pic = a.getByRole('button', { name: /^Show image: Fort Joy map, from wiki\.example\.com/ });
  await expect(pic).toBeVisible();
  await expect(pic).toContainText('Show image');
  await expect(pic).toContainText('wiki.example.com');
  await main.waitForTimeout(500);
  expect(pictureAsked(fakeWeb)).toBe(false);
  await pic.click();
  const img = a.locator('img.md-img');
  await expect(img).toBeVisible();
  await expect.poll(() => img.evaluate((el) => el.naturalWidth)).toBe(320);
  expect(await img.getAttribute('alt')).toBe('Fort Joy map');
  expect(await img.getAttribute('src')).toMatch(/^blob:/);
  expect(pictureAsked(fakeWeb)).toBe(true);
  // Listed in the network receipts, by its purpose and host.
  await expectStore(yap, 'connections.json', (s) => s.entries.some((e) => e.label === 'A picture from the web' && e.host === 'wiki.example.com'));
  await shot(main, '03-answer-picture-light');

  // Dark: the diagram drawn again in the theme's colours.
  await theme(yap, main, 'dark');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  await expect.poll(async () => (await diagramIn(box))?.fill).not.toBe(light.fill);
  await shot(main, '04-answer-dark');
  await theme(yap, main, 'light');
});

/** The KaTeX fonts the window has loaded (from the maths chunk's CSS), as "family style weight". */
const katexFonts = (main) =>
  main.evaluate(async () => {
    await document.fonts.ready;
    return [...document.fonts]
      .map((f) => ({ family: f.family.replace(/["']/g, ''), style: f.style, weight: f.weight, status: f.status }))
      .filter((f) => f.family.startsWith('KaTeX_') && f.status === 'loaded')
      .map((f) => `${f.family} ${f.style} ${f.weight}`);
  });

test("KaTeX's fonts and CSS load, and a diagram with maths in its labels still draws, light and dark", async ({ yap, main, shot }) => {
  await theme(yap, main, 'light');
  await openView(main, 'Chat');
  await main.locator('.conv').filter({ hasText: 'Maths in a diagram' }).click();
  const a = answer(main);
  await expect(a.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(3);
  await expect(a.locator('.md-math-display .katex-display')).toHaveCount(1);

  // KaTeX's sheet applies (its own font on the formula, a fraction laid out)
  // and its fonts load from the chunk, under Yap's own scheme.
  expect(await a.locator('.md-math .katex').first().evaluate((el) => getComputedStyle(el).fontFamily)).toContain('KaTeX_Main');
  await expect(a.locator('.md-math .katex .mfrac').first()).toBeVisible();
  await expect.poll(() => katexFonts(main), { timeout: 10_000 }).toEqual(
    expect.arrayContaining([expect.stringMatching(/^KaTeX_Main normal/), expect.stringMatching(/^KaTeX_Math italic/), expect.stringMatching(/^KaTeX_Size\d/)])
  );

  // The diagram draws with maths in a node and on an edge. (Mermaid draws
  // that maths with KaTeX as MathML; the label rules, ui/diagrams.js
  // `LABELS`, keep text tags only, so it's left blank: never its TeX.)
  const box = a.locator('.md-diagram');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  const light = await diagramIn(box);
  expect(light.labels).toContain('Root');
  expect(light.labels).not.toContain('$$');
  await shot(main, '08-maths-and-diagram-light');

  await theme(yap, main, 'dark');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  await expect.poll(async () => (await diagramIn(box))?.fill).not.toBe(light.fill);
  await expect(a.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(3);
  await shot(main, '09-maths-and-diagram-dark');
  await theme(yap, main, 'light');
});

test("a note's summary draws its maths and its diagram", async ({ yap, main, shot }) => {
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Physics review' }).click();
  const summary = main.locator('.rendered').first();
  // (The Enhanced tab, if the note opens on its own words.)
  if (!(await summary.isVisible().catch(() => false))) await main.locator('.tabs .tab').filter({ hasText: /Summary|Enhanced/ }).first().click();
  await expect(summary.locator('.md-math[data-drawn="true"] .katex')).toHaveCount(1);
  const box = summary.locator('.md-diagram');
  await expect(box).toHaveAttribute('data-drawn', 'true', { timeout: 20_000 });
  expect((await diagramIn(box)).labels).toContain('Review');
  await shot(main, '05-note-summary');
});

test("the page's editor draws them too, and the note stays as written", async ({ yap, main, shot }) => {
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Physics review' }).click();
  const page = main.getByRole('textbox', { name: 'My thoughts' });
  await expect(page).toBeVisible();
  // Maths drawn in its line, its text hidden; the price is text.
  await expect(page.locator('.rich-math .katex')).toHaveCount(1);
  await expect(page.locator('.rich-hidden-source').first()).toBeHidden();
  await expect(page).toContainText('at $5 a slide');
  // The diagram above its code, the code hidden until the caret is in it.
  const diagram = page.locator('.rich-diagram');
  await expect(diagram).toHaveCount(1, { timeout: 20_000 });
  expect((await diagramIn(diagram)).labels).toContain('Check');
  const code = page.locator('pre');
  await expect(code).toBeHidden();
  await shot(main, '06-editor-drawn');
  await diagram.locator('.md-diagram-view').click();
  await expect(code).toBeVisible();
  await expect(code).toContainText('Draft --> Check --> Ship');
  await shot(main, '07-editor-code');
  // Out of it: the code hides again. Nothing was written: the note is as it was.
  await page.getByText('Then the review.').click();
  await expect(code).toBeHidden();
  await main.waitForTimeout(800);
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === 41)?.content === THOUGHTS, 'the note as written');
});
