// Files shared into the conversation (`share_files`: desktop/src-tauri/src/chat/shares.rs,
// desktop/src/lib/views/chat/SharedFiles.svelte, shares.js; after the Claude app's
// SendUserFile with display "render"):
// - One picture: copied into chat-images/ as a pasted one is, sent to the page the moment
//   it lands (`{kind: "share"}`), kept with the answer (`files` in chats.json), drawn large in
//   the thread with its caption; a click opens the viewer while the panel's closed, and the
//   panel's picture tab while it's open.
// - Three pictures: a grid, each tile at its device pixels.
// - A Markdown file and a .txt: cards, linked by their path (never copied) — Open (the
//   Markdown file in Yap's own window, the text file in its app: kept for the suite in a
//   test run), Show in folder (kept too), Copy path, the preview folded under ▸; a file
//   deleted since says "Moved or deleted".
// - An agent's report card (seeded): the pictures its turn made, four then "+N more", and
//   its new files as compact cards.
// - An older Yap's save dropped `files`: chat_files.json puts them back at the start.
// Light and dark. The model is the scripted fake AI (support/fake-llm.js) with the web off:
// a question naming a picture file on this PC gets the picture files' gate
// (pictures/local.rs) and share_files with it. Nothing leaves the PC; nothing is opened
// outside Yap (`e2e_opened_urls`). Fictional files only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, setConfig, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { encode } from './support/sharpness.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'chat-share-spec');
const GARDEN = path.join(HERE, 'garden-planner');
const DOCS = path.join(GARDEN, 'docs');
const file = (name) => path.join(DOCS, name);
const CONCEPT = file('concept.png');
const NOTES = file('notes.md');
const TODO = file('todo.txt');
const CAPTION = 'The first garden concept';
const NOW = Math.floor(Date.now() / 1000);

/** A made-up picture: beds in rows on grass, a path through them. */
function gardenPicture(width, height, hue = 0) {
  const data = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let c = [96 + hue, 160, 88];
      if (Math.floor(y / 40) % 2 === 0 && x % 120 > 16) c = [120, 86 + hue, 56];
      if (Math.abs(x - width / 2) < 10) c = [214, 204, 176];
      data.set([...c, 255], (y * width + x) * 4);
    }
  }
  return encode({ width, height, data });
}

// The report card's (seeded) pictures in chat-images, and its agent's files.
const REPORT_IDS = ['img_5ea1000000000001', 'img_5ea1000000000002', 'img_5ea1000000000003', 'img_5ea1000000000004'];
const ART = path.join(GARDEN, 'art');
const reportPicture = (i) => ({ name: `concept-${i + 1}.png`, path: path.join(ART, `concept-${i + 1}.png`), folder: ART, kind: 'image', bytes: 2048, id: REPORT_IDS[i], w: 320, h: 200 });
const REPORT_FILES = {
  items: [
    ...REPORT_IDS.map((_, i) => reportPicture(i)),
    { name: 'plan.md', path: path.join(GARDEN, 'plan.md'), folder: GARDEN, kind: 'markdown', bytes: 120 },
    { name: 'layout.txt', path: path.join(GARDEN, 'layout.txt'), folder: GARDEN, kind: 'text', bytes: 64 },
  ],
  morePictures: 2,
};
// An answer whose files an older Yap's save dropped from chats.json: kept in chat_files.json.
const OLDER = { caption: 'Notes for the beds', items: [{ name: 'notes.md', path: NOTES, folder: DOCS, kind: 'markdown', bytes: 60 }] };

const CHATS = {
  conversations: [
    {
      id: 71,
      title: 'Garden agent',
      createdTs: NOW - 900,
      updatedTs: NOW - 880,
      messages: [
        { role: 'user', text: 'Make me some concept pictures for the garden planner', ts: NOW - 900 },
        { role: 'assistant', text: 'I’ve sent an agent to draw them.', ts: NOW - 899 },
        {
          role: 'user',
          kind: 'agentReport',
          text: "The agent in garden-planner: it's done.\nFiles it changed: concept-1.png, concept-2.png and 6 more (backed up first).",
          ts: NOW - 890,
          files: [REPORT_FILES],
        },
        { role: 'assistant', text: 'Done. The agent drew six concepts and wrote a plan.', ts: NOW - 889 },
      ],
    },
    {
      id: 72,
      title: 'Bed notes',
      createdTs: NOW - 600,
      updatedTs: NOW - 590,
      messages: [
        { role: 'user', text: `What's in ${DOCS}?`, ts: NOW - 600 },
        { role: 'assistant', text: 'Here are the notes.', ts: NOW - 599 },
      ],
    },
  ],
};
const MIRROR = { version: 1, messages: [{ conversation: 72, index: 1, ts: NOW - 599, files: [OLDER] }] };

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
      // The files on this PC the chats share (fictional).
      fs.rmSync(HERE, { recursive: true, force: true });
      fs.mkdirSync(DOCS, { recursive: true });
      fs.mkdirSync(ART, { recursive: true });
      fs.writeFileSync(CONCEPT, gardenPicture(640, 400));
      fs.writeFileSync(file('beds.png'), gardenPicture(400, 300, 40));
      fs.writeFileSync(file('paths.png'), gardenPicture(300, 300, 80));
      fs.writeFileSync(NOTES, '# Garden plan\n\nTomatoes by the south fence.\nBeans along the north wall.\n');
      fs.writeFileSync(TODO, 'Buy seeds\nTurn the compost\nMend the gate\n');
      fs.writeFileSync(path.join(GARDEN, 'plan.md'), '# Plan\n\nSix beds, one path.\n');
      fs.writeFileSync(path.join(GARDEN, 'layout.txt'), 'North: peas\nSouth: tomatoes\n');
      REPORT_IDS.forEach((_, i) => fs.writeFileSync(path.join(ART, `concept-${i + 1}.png`), gardenPicture(320, 200, i * 20)));
      await use({
        name: 'chat-share',
        data: { 'chats.json': CHATS, 'chat_files.json': MIRROR },
        config: {
          webSearch: false,
          meetingReminder: 'never',
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
        prepare: (dir) => {
          const images = path.join(dir, 'chat-images');
          fs.mkdirSync(images, { recursive: true });
          REPORT_IDS.forEach((id, i) => fs.writeFileSync(path.join(images, `${id}.png`), gardenPicture(320, 200, i * 20)));
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const sharedPictures = (main) => main.getByTestId('chat-shared-picture');
const cards = (main) => main.getByTestId('chat-file-card');
const card = (main, name) => cards(main).filter({ has: main.getByTestId('chat-file-name').getByText(name, { exact: true }) });

/** A new chat, its question sent. */
async function ask(main, text) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The conversation `title` open. */
async function openConversation(main, title) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  await main.locator('.conv').filter({ hasText: title }).click();
}

async function loaded(img) {
  await expect.poll(() => img.evaluate((el) => el.complete && el.naturalWidth > 0), { message: 'the picture to load' }).toBe(true);
}

/** An <img>'s box on screen (CSS px), its natural size and the pixel ratio. */
const placement = (img) =>
  img.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { w: r.width, h: r.height, nw: el.naturalWidth, nh: el.naturalHeight, src: el.getAttribute('src'), dpr: window.devicePixelRatio };
  });

const answerWith = (s, id, pred) => s.conversations.find((c) => c.id === id)?.messages.find((m) => m.role === 'assistant' && pred(m));

/** The windows of Markdown files open now. */
async function docLabels(yap) {
  const out = [];
  for (const page of yap.browser.contexts()[0].pages()) {
    const l = await page.evaluate(() => window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? null).catch(() => null);
    if (l?.startsWith('doc-')) out.push(l);
  }
  return out;
}

test('one picture: copied, kept, large in the thread; a click opens the viewer, or the panel’s tab', async ({ yap, main, fakeLlm, shot }) => {
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'share_files', args: { files: [CONCEPT], caption: CAPTION } }] },
    { tools: [{ name: 'show_picture', args: { path: file('beds.png'), caption: 'The beds' } }] },
    { content: 'Here is the first concept, and the beds beside the chat.' },
  ]);
  await ask(main, `Can you show me ${CONCEPT}?`);
  const pic = sharedPictures(main);
  await expect(pic).toHaveCount(1, { timeout: 20_000 });
  await expect(main.getByTestId('chat-share-caption')).toHaveText(CAPTION);
  await expect(main.getByText('Here is the first concept')).toBeVisible();
  const img = pic.locator('img');
  await loaded(img);
  // Large: as wide as it is (640 px fits the column), its shape kept, at its device pixels.
  const p = await placement(img);
  expect(p.w).toBeGreaterThan(300);
  expect(p.h).toBeLessThanOrEqual(480);
  expect(Math.abs(p.w / p.h - 640 / 400)).toBeLessThan(0.02);
  expect(Math.abs(p.w * p.dpr - p.nw)).toBeLessThan(0.6);

  // The model was offered it (the web off: a picture file named), by a path only.
  const offered = fakeLlm.requests.slice(from).find((r) => r.tools?.length);
  expect(offered.tools).toContain('share_files');
  const result = JSON.parse(fakeLlm.requests.slice(from).filter((r) => r.tools?.length)[1].messages.filter((m) => m.role === 'tool').at(-1).content);
  expect(result).toMatchObject({ success: true, displayText: 'Shared concept.png', data: { shared: 'Shared concept.png in the chat' } });

  // Copied into chat-images as a pasted picture, kept with the answer.
  let id = null;
  await expectStore(yap, 'chats.json', (s) => {
    const answer = s.conversations.flatMap((c) => c.messages).find((m) => m.files?.[0]?.caption === CAPTION);
    id = answer?.files[0].items[0].id ?? null;
    return !!id && answer.files[0].items[0].path.toLowerCase().endsWith('concept.png');
  });
  expect(id).toMatch(/^img_[0-9a-f]{16}$/);
  expect(fs.existsSync(path.join(yap.dataDir, 'chat-images', `${id}.png`))).toBe(true);
  // …and in the copy for an older Yap's save.
  await expectStore(yap, 'chat_files.json', (s) => s.messages.some((m) => m.files[0].caption === CAPTION));

  // The panel's open (show_picture of the beds): the click shows it there.
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('beds.png');
  await shot(main, '01-one-picture-panel-open');
  await pic.click();
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('concept.png');
  await expect(main.getByTestId('chat-picture-source')).toContainText('docs');
  // Closed: the viewer.
  await main.getByTestId('chat-picture').getByRole('button', { name: 'Close the panel' }).click();
  await expect(main.getByTestId('chat-picture')).toHaveCount(0);
  await pic.click();
  const viewer = main.getByRole('dialog', { name: 'Screenshot' });
  await expect(viewer).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
  // Its ⋯: Copy / Save / Show in folder / Open original.
  await pic.hover();
  await main.getByTestId('chat-shared-picture-more').click();
  const menu = main.getByRole('menu');
  for (const item of ['Copy image', 'Save image as…', 'Show in folder', 'Open original']) {
    await expect(menu.getByRole('menuitem', { name: item })).toBeVisible();
  }
  await menu.getByRole('menuitem', { name: 'Show in folder' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual(expect.stringMatching(/^reveal:.*concept\.png$/i));
  await shot(main, '02-one-picture');
});

test('three pictures: a grid, each at its device pixels', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'share_files', args: { files: [CONCEPT, file('beds.png'), file('paths.png')], caption: 'Three ideas for the garden' } }] },
    { content: 'Three ideas.' },
  ]);
  await ask(main, `Show me the pictures next to ${CONCEPT}`);
  const grid = main.getByTestId('chat-shared-grid');
  await expect(grid).toBeVisible({ timeout: 20_000 });
  await expect(grid.getByTestId('chat-shared-picture')).toHaveCount(3);
  await expect(main.getByTestId('chat-share-caption').last()).toHaveText('Three ideas for the garden');
  // Two columns, square tiles, each picture at exactly its device pixels.
  const tiles = grid.getByTestId('chat-shared-picture');
  const boxes = [];
  for (let i = 0; i < 3; i++) {
    const img = tiles.nth(i).locator('img');
    await loaded(img);
    const p = await placement(img);
    expect(Math.abs(p.w * p.dpr - p.nw)).toBeLessThan(0.6);
    boxes.push(await tiles.nth(i).evaluate((el) => el.closest('.sharedpic').getBoundingClientRect().toJSON()));
  }
  expect(Math.abs(boxes[0].y - boxes[1].y)).toBeLessThan(1);
  expect(boxes[2].y).toBeGreaterThan(boxes[0].y + boxes[0].height - 1);
  expect(Math.abs(boxes[0].width - boxes[0].height)).toBeLessThan(1);
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.files?.[0]?.items?.length === 3)));
  await shot(main, '03-three-pictures-grid');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '04-three-pictures-grid-dark');
  await setConfig(yap, main, { theme: 'light' });
});

test('a Markdown file and a text file: cards that open, show their folder and copy their path; one deleted says so', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([{ tools: [{ name: 'share_files', args: { files: [NOTES, TODO], caption: 'Your garden notes' } }] }, { content: 'Here are your notes and the to-do list.' }]);
  await ask(main, `Send me the notes beside ${CONCEPT}`);
  await expect(card(main, 'notes.md')).toBeVisible({ timeout: 20_000 });
  await expect(card(main, 'todo.txt')).toBeVisible();
  // Linked, never copied: no chat-images id, the path kept.
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.files?.[0]?.caption === 'Your garden notes' && m.files[0].items.every((i) => !i.id && i.path))));
  const notes = card(main, 'notes.md');
  await expect(notes).toContainText('Markdown');
  await expect(notes).toContainText(/\d+ B/);
  await expect(notes.locator('.ffolder')).toHaveAttribute('title', DOCS);
  await expect(card(main, 'todo.txt')).toContainText('Text');
  // The preview, folded under ▸: read from the file when it's unfolded.
  await notes.getByTestId('chat-file-preview').locator('summary').click();
  await expect(notes.getByTestId('chat-file-preview-text')).toContainText('Tomatoes by the south fence.');
  await shot(main, '05-file-cards');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '06-file-cards-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Copy path: the file's whole path on the clipboard.
  await main.evaluate(() => {
    window.__copied = [];
    navigator.clipboard.writeText = async (t) => void window.__copied.push(t);
  });
  await notes.getByTestId('chat-file-copy-path').click();
  await expect.poll(() => main.evaluate(() => window.__copied)).toEqual([NOTES]);
  // Show in folder, and Open for the text file: kept for the suite, never opened in a test run.
  await card(main, 'todo.txt').getByTestId('chat-file-reveal').click();
  await card(main, 'todo.txt').getByTestId('chat-file-open').click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toEqual(expect.arrayContaining([`reveal:${TODO}`, `open:${TODO}`]));
  // Open for the Markdown file: Yap's own window.
  const before = await docLabels(yap);
  await notes.getByTestId('chat-file-open').click();
  await expect.poll(async () => (await docLabels(yap)).filter((l) => !before.includes(l)).length, { timeout: 20_000 }).toBe(1);
  expect(await yap.invoke('e2e_opened_urls')).not.toContainEqual(expect.stringContaining('notes.md'));

  // Deleted since: the card says so, plainly, when the chat's opened again.
  fs.rmSync(TODO);
  await openConversation(main, 'Bed notes');
  await openConversation(main, `Send me the notes beside`);
  await expect(card(main, 'todo.txt').getByTestId('chat-file-gone')).toHaveText('Moved or deleted');
  await expect(card(main, 'todo.txt').getByTestId('chat-file-open')).toHaveCount(0);
  await expect(card(main, 'notes.md').getByTestId('chat-file-gone')).toHaveCount(0);
  await shot(main, '07-file-moved-or-deleted');
  fs.writeFileSync(TODO, 'Buy seeds\n');
});

test('an agent’s report card shows the pictures its turn made, then +N more, and its new files', async ({ main, shot }) => {
  await openConversation(main, 'Garden agent');
  const report = main.getByTestId('chat-agent-report');
  await expect(report).toBeVisible();
  const grid = main.getByTestId('chat-shared-grid');
  await expect(grid.getByTestId('chat-shared-picture')).toHaveCount(4);
  await expect(main.getByTestId('chat-shared-more')).toHaveText('+2 more');
  await loaded(grid.getByTestId('chat-shared-picture').first().locator('img'));
  await expect(card(main, 'plan.md')).toBeVisible();
  await expect(card(main, 'layout.txt')).toBeVisible();
  // Compact: no preview, no Copy path.
  await expect(card(main, 'plan.md').getByTestId('chat-file-preview')).toHaveCount(0);
  await expect(card(main, 'plan.md').getByTestId('chat-file-copy-path')).toHaveCount(0);
  await shot(main, '08-agent-report');
});

test('an older Yap’s save dropped an answer’s files: chat_files.json put them back', async ({ main }) => {
  await openConversation(main, 'Bed notes');
  await expect(card(main, 'notes.md')).toBeVisible();
  await expect(main.getByTestId('chat-share-caption')).toHaveText('Notes for the beds');
});
