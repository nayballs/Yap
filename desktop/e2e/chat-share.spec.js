// Files shared into the conversation (`share_files`: desktop/src-tauri/src/chat/shares.rs,
// desktop/src/lib/views/chat/SharedFiles.svelte, shares.js; after the Claude app's
// SendUserFile with display "render"):
// - One picture: copied into chat-images/ as a pasted one is, sent to the page the moment
//   it lands (`{kind: "share"}`), kept with the answer (`files` in chats.json), drawn large in
//   the thread with its caption; a click opens the viewer while the panel's closed, and the
//   panel's picture tab while it's open.
// - Three pictures: a grid, each tile at its device pixels.
// - A Markdown file and a .txt: chips (16 Nov 2026, the Claude app's: a coloured type badge,
//   the name, the size), linked by their path (never copied), side by side — a click opens
//   one beside the chat in Yap's file view (`chat_file_read`: no "▸ Preview" any more), its ⋯
//   (on hover or keyboard focus, and its right-click) the menu: Open in Yap, Open in its own
//   window / its app (kept for the suite in a test run), Show in folder (kept too), Copy
//   path; a file deleted since says "Moved or deleted" and its menu only Copy path.
// - Six files: chips in a row that wraps, none past the column; a .csv opens in the sheet
//   view, read only.
// - An agent's report card (seeded): the pictures its turn made, four then "+N more", and
//   its new files as chips.
// - One chip a file a turn (seeded): a report's file the answer shares too is its chip under
//   the answer and only a link in the report's "Changed …" line, which opens it.
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
import { badgeOf, familyOf, nameParts, fileSize, normPath, chippedElsewhere, onceEach, changedParts } from '../src/lib/views/chat/fileChip.js';

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
// A report whose new file the answer after it shares too: one chip, under the answer.
const PLAN = { name: 'plan.md', path: path.join(GARDEN, 'plan.md'), folder: GARDEN, kind: 'markdown', bytes: 31 };
// Six files of every family, for the row that wraps (fictional, made below).
const MANY = ['north-border-bed-measurements.csv', 'seed-order-for-the-spring.pdf', 'watering-schedule.md', 'compost-notes.txt', 'tool-shed-inventory.xlsx', 'garden-photos.zip'];

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
      id: 73,
      title: 'Bed plan agent',
      createdTs: NOW - 800,
      updatedTs: NOW - 790,
      messages: [
        { role: 'user', text: 'Write me a plan for the beds and send it over', ts: NOW - 800 },
        {
          role: 'user',
          kind: 'agentReport',
          text: "The agent in garden-planner: it's done.\nFiles it changed: plan.md (new: Undo deletes them).",
          ts: NOW - 795,
          files: [{ items: [PLAN] }],
        },
        { role: 'assistant', text: 'Here is the plan for the beds.', ts: NOW - 794, files: [{ caption: 'The bed plan', items: [PLAN] }] },
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
      MANY.forEach((n, i) => fs.writeFileSync(file(n), n.endsWith('.csv') ? 'bed,length cm\nnorth 1,240\nnorth 2,180\n' : `${n}\n`.repeat(i * 40 + 1)));
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
const menu = (main) => main.getByRole('menu');
/** A chip's ⋯ menu, opened with the pointer on the chip. */
async function chipMenu(main, chip) {
  await chip.hover();
  await expect(chip.getByTestId('chat-file-more')).toBeVisible();
  await chip.getByTestId('chat-file-more').click();
  await expect(menu(main)).toBeVisible();
  return menu(main);
}

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
  // (Its folder named too: a file's path alone allows that file only, never its neighbours.)
  await ask(main, `Can you show me ${CONCEPT}? The beds are in ${DOCS}`);
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
  // Whole, never cropped: its frame is its own shape (no bars), the picture filling it.
  const frame = await pic.evaluate((el) => el.closest('.sharedpic').getBoundingClientRect().toJSON());
  expect(Math.abs(frame.width - p.w)).toBeLessThan(1);
  expect(Math.abs(frame.height - p.h)).toBeLessThan(1);
  expect(p.src).not.toContain('fit=cover');

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
  await ask(main, `Show me ${CONCEPT} and the others in ${DOCS}`);
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
    // Filled: Rust's tile of the picture's middle at exactly its device pixels.
    expect(p.src).toMatch(/\?w=\d+&h=\d+&fit=cover$/);
    expect(Math.abs(p.h * p.dpr - p.nh)).toBeLessThan(0.6);
    boxes.push(await tiles.nth(i).evaluate((el) => el.closest('.sharedpic').getBoundingClientRect().toJSON()));
  }
  expect(Math.abs(boxes[0].y - boxes[1].y)).toBeLessThan(1);
  expect(boxes[2].y).toBeGreaterThan(boxes[0].y + boxes[0].height - 1);
  // 4:3 tiles, the picture filling each (no bars).
  expect(Math.abs(boxes[0].width / boxes[0].height - 4 / 3)).toBeLessThan(0.02);
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.files?.[0]?.items?.length === 3)));
  await shot(main, '03-three-pictures-grid');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '04-three-pictures-grid-dark');
  await setConfig(yap, main, { theme: 'light' });
});

test('a Markdown file and a text file: chips that open beside the chat, a ⋯ menu each; one deleted says so', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([{ tools: [{ name: 'share_files', args: { files: [NOTES, TODO], caption: 'Your garden notes' } }] }, { content: 'Here are your notes and the to-do list.' }]);
  await ask(main, `Send me the notes in ${DOCS} with ${CONCEPT}`);
  await expect(card(main, 'notes.md')).toBeVisible({ timeout: 20_000 });
  await expect(card(main, 'todo.txt')).toBeVisible();
  // Linked, never copied: no chat-images id, the path kept.
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.files?.[0]?.caption === 'Your garden notes' && m.files[0].items.every((i) => !i.id && i.path))));
  const notes = card(main, 'notes.md');
  // The Claude app's chip: its type, its name, its size — nothing else on it (no buttons, no preview).
  await expect(notes.getByTestId('chat-file-badge')).toHaveText('MD');
  await expect(notes).toHaveAttribute('data-family', 'doc');
  await expect(notes.getByTestId('chat-file-size')).toHaveText(/^\d+B$/);
  await expect(card(main, 'todo.txt').getByTestId('chat-file-badge')).toHaveText('TXT');
  await expect(notes.getByRole('button')).toHaveCount(2); // itself and its ⋯
  await expect(main.getByTestId('chat-file-preview')).toHaveCount(0);
  await expect(notes.getByTestId('chat-file-open')).toHaveAttribute('title', NOTES);
  // Side by side, the caption a quiet line under them.
  const [n, t] = await Promise.all([notes.boundingBox(), card(main, 'todo.txt').boundingBox()]);
  expect(Math.abs(n.y - t.y)).toBeLessThan(1);
  expect(t.x).toBeGreaterThan(n.x + n.width);
  const caption = main.getByTestId('chat-share-caption').last();
  await expect(caption).toHaveText('Your garden notes');
  expect((await caption.boundingBox()).y).toBeGreaterThan(n.y + n.height);
  // The Claude app's proportions at the default reading size: 36 px tall, an 18 px badge, the name 13.5 px.
  const size = await notes.evaluate((el) => ({
    h: el.getBoundingClientRect().height,
    badge: el.querySelector('[data-testid="chat-file-badge"]').getBoundingClientRect().height,
    name: parseFloat(getComputedStyle(el.querySelector('[data-testid="chat-file-name"]')).fontSize),
  }));
  expect(size.h).toBeGreaterThanOrEqual(35);
  expect(size.h).toBeLessThanOrEqual(37);
  expect(size.badge).toBeCloseTo(18, 0);
  expect(size.name).toBeCloseTo(13.5, 1);
  // No ⋯ until the pointer or the keyboard is on it.
  await expect(notes.getByTestId('chat-file-more')).toHaveCSS('opacity', '0');
  await shot(main, '05-file-chips');
  await notes.hover();
  await expect(notes.getByTestId('chat-file-more')).toHaveCSS('opacity', '1');
  await shot(main, '05b-file-chip-hover');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '06-file-chips-dark');
  await setConfig(yap, main, { theme: 'light' });

  // A click: the file beside the chat, in Yap's file view (its preview), read for a file shared in this chat only.
  await notes.getByTestId('chat-file-open').click();
  const view = main.getByTestId('chat-file');
  await expect(view).toHaveAttribute('data-rel', NOTES.replace(/\\/g, '/'));
  await expect(view.getByTestId('chat-file-name')).toHaveText('notes.md');
  await expect(view.getByTestId('chat-file-body')).toContainText('Tomatoes by the south fence.');
  await expect(view.getByTestId('chat-file-open-app')).toHaveText('Open in its own window');
  await expect(view.getByTestId('chat-file-open-editor')).toHaveCount(0);
  await shot(main, '06b-a-shared-file-beside-the-chat');
  await main.getByTestId('chat-doc').getByRole('button', { name: 'Close the panel' }).first().click();

  // Its ⋯ by the keyboard: Tab from the chip, Enter; the menu as a file link's.
  await main.evaluate(() => {
    window.__copied = [];
    navigator.clipboard.writeText = async (t) => void window.__copied.push(t);
  });
  await notes.getByTestId('chat-file-open').focus();
  await main.keyboard.press('Tab');
  await expect(notes.getByTestId('chat-file-more')).toBeFocused();
  await expect(notes.getByTestId('chat-file-more')).toHaveCSS('opacity', '1');
  await main.keyboard.press('Enter');
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Open in Yap', 'Open in its own window', 'Show in folder', 'Copy path']);
  await shot(main, '06c-file-chip-menu');
  await menu(main).getByRole('menuitem', { name: 'Copy path' }).click();
  await expect.poll(() => main.evaluate(() => window.__copied)).toEqual([NOTES]);
  // Show in folder and Open in its app for the text file (its right-click): kept for the suite, never opened in a test run.
  await card(main, 'todo.txt').click({ button: 'right' });
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Open in Yap', 'Open in its app', 'Show in folder', 'Copy path']);
  await menu(main).getByRole('menuitem', { name: 'Show in folder' }).click();
  await (await chipMenu(main, card(main, 'todo.txt'))).getByRole('menuitem', { name: 'Open in its app' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toEqual(expect.arrayContaining([`reveal:${TODO}`, `open:${TODO}`]));
  // Open in its own window for the Markdown file: Yap's own window.
  const before = await docLabels(yap);
  await (await chipMenu(main, notes)).getByRole('menuitem', { name: 'Open in its own window' }).click();
  await expect.poll(async () => (await docLabels(yap)).filter((l) => !before.includes(l)).length, { timeout: 20_000 }).toBe(1);
  expect(await yap.invoke('e2e_opened_urls')).not.toContainEqual(expect.stringContaining('notes.md'));

  // Deleted since: the chip says so, plainly, when the chat's opened again; a click opens nothing, its menu only Copy path.
  fs.rmSync(TODO);
  await openConversation(main, 'Bed notes');
  await openConversation(main, `Send me the notes in`);
  const gone = card(main, 'todo.txt');
  await expect(gone.getByTestId('chat-file-gone')).toHaveText('Moved or deleted');
  await expect(notes.getByTestId('chat-file-gone')).toHaveCount(0);
  await expect(gone.getByTestId('chat-file-open')).toHaveAttribute('aria-disabled', 'true');
  await gone.getByTestId('chat-file-open').click({ force: true });
  await expect(main.getByTestId('chat-file')).toHaveCount(0);
  await expect((await chipMenu(main, gone)).getByRole('menuitem')).toHaveText(['Copy path']);
  await main.keyboard.press('Escape');
  await shot(main, '07-file-moved-or-deleted');
  fs.writeFileSync(TODO, 'Buy seeds\n');
});

test('six files: chips in a row that wraps, none past the column; a .csv opens in the sheet view, read only', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([{ tools: [{ name: 'share_files', args: { files: MANY.map(file), caption: 'Everything for the beds' } }] }, { content: 'Here is everything for the beds.' }]);
  await ask(main, `Send me everything in ${DOCS} with ${CONCEPT}`);
  const row = main.getByTestId('chat-file-chips').last();
  await expect(row.getByTestId('chat-file-card')).toHaveCount(6, { timeout: 20_000 });
  await expect(row.getByTestId('chat-file-badge')).toHaveText(['CSV', 'PDF', 'MD', 'TXT', 'XLSX', 'ZIP']);
  expect(await row.getByTestId('chat-file-card').evaluateAll((els) => els.map((el) => el.dataset.family))).toEqual(['sheet', 'pdf', 'doc', 'text', 'sheet', 'archive']);
  // Each badge its family's colour, its letters on a wash of it (app.css --yap-file-*).
  const colours = await row.getByTestId('chat-file-badge').evaluateAll((els) => els.map((el) => getComputedStyle(el).color));
  expect(new Set(colours).size).toBe(5);
  const boxes = await row.getByTestId('chat-file-card').evaluateAll((els) =>
    els.map((el) => {
      const r = el.getBoundingClientRect();
      return { top: Math.round(r.top), right: r.right };
    }),
  );
  const column = await main.getByTestId('chat-column').evaluate((el) => el.getBoundingClientRect().right);
  expect(new Set(boxes.map((b) => b.top)).size).toBeGreaterThan(1);
  for (const b of boxes) expect(b.right).toBeLessThanOrEqual(column + 0.5);
  await shot(main, '07b-six-chips-wrap');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '07c-six-chips-wrap-dark');
  await setConfig(yap, main, { theme: 'light' });
  // A long name is cut in the middle: its ending shows.
  const long = card(main, 'north-border-bed-measurements.csv');
  await expect(long.locator('.ftail')).toHaveText(/\.csv$/);
  // The .csv: the sheet view, read only (a file shared from outside a Code chat's folder).
  await long.getByTestId('chat-file-open').click();
  const sheet = main.getByTestId('chat-sheet');
  await expect(sheet).toHaveAttribute('data-name', 'north-border-bed-measurements.csv');
  await expect(sheet.getByTestId('chat-sheet-readonly')).toBeVisible();
  await expect(sheet).toContainText('north 1');
  await shot(main, '07d-a-shared-csv-as-a-sheet');
  await main.getByTestId('chat-doc').getByRole('button', { name: 'Close the panel' }).first().click();
  // A PDF has no view of Yap's: its click opens it in its app (kept for the suite).
  await card(main, 'seed-order-for-the-spring.pdf').getByTestId('chat-file-open').click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual(`open:${file('seed-order-for-the-spring.pdf')}`);
});

test('an agent’s report card shows the pictures its turn made, then +N more, and its new files', async ({ yap, main, shot }) => {
  await openConversation(main, 'Garden agent');
  const report = main.getByTestId('chat-agent-report');
  await expect(report).toBeVisible();
  const grid = main.getByTestId('chat-shared-grid');
  await expect(grid.getByTestId('chat-shared-picture')).toHaveCount(4);
  await expect(main.getByTestId('chat-shared-more')).toHaveText('+2 more');
  await loaded(grid.getByTestId('chat-shared-picture').first().locator('img'));
  await expect(card(main, 'plan.md')).toBeVisible();
  await expect(card(main, 'layout.txt')).toBeVisible();
  // Chips, as an answer's: side by side, a badge each.
  await expect(main.getByTestId('chat-file-chips').getByTestId('chat-file-badge')).toHaveText(['MD', 'TXT']);
  // Each tile filled with its picture (4:3, Rust's tile at its device pixels: no bars).
  const tile = await grid.getByTestId('chat-shared-picture').first().evaluate((el) => {
    const r = el.closest('.sharedpic').getBoundingClientRect();
    return { ratio: r.width / r.height, src: el.querySelector('img').getAttribute('src') };
  });
  expect(Math.abs(tile.ratio - 4 / 3)).toBeLessThan(0.02);
  expect(tile.src).toContain('fit=cover');
  await shot(main, '08-agent-report');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '09-agent-report-dark');
  await setConfig(yap, main, { theme: 'light' });
});

test('one chip a file a turn: a report’s file the answer shares is its chip under the answer, a link in the report’s line', async ({ yap, main, shot }) => {
  await openConversation(main, 'Bed plan agent');
  const report = main.getByTestId('chat-agent-report');
  await expect(report).toBeVisible();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('Here is the plan for the beds.');
  // One chip, the answer's (under it, with its caption); none on the report.
  await expect(card(main, 'plan.md')).toHaveCount(1);
  await expect(main.getByTestId('chat-share-caption')).toHaveText('The bed plan');
  const chip = await card(main, 'plan.md').boundingBox();
  expect(chip.y).toBeGreaterThan((await main.locator('.cbubble.assistant').last().boundingBox()).y);
  // The report names it as a link: it opens it as its chip would.
  const link = report.getByTestId('chat-agent-report-file');
  await expect(link).toHaveText('plan.md');
  await expect(report.getByTestId('chat-agent-report-changed')).toHaveText('Changed plan.md');
  await shot(main, '09b-one-chip-a-file-a-turn');
  await link.click();
  const view = main.getByTestId('chat-file');
  await expect(view).toHaveAttribute('data-rel', PLAN.path.replace(/\\/g, '/'));
  await expect(view.getByTestId('chat-file-body')).toContainText('Six beds, one path.');
  await main.getByTestId('chat-doc').getByRole('button', { name: 'Close the panel' }).first().click();
  // Its right-click: the chip's menu.
  await link.click({ button: 'right' });
  await expect(menu(main).getByRole('menuitem')).toHaveText(['Open in Yap', 'Open in its own window', 'Show in folder', 'Copy path']);
  await main.keyboard.press('Escape');
  // A file no other chip shows stays a chip on the report (the 'Garden agent' chat's).
  await openConversation(main, 'Garden agent');
  await expect(card(main, 'plan.md')).toHaveCount(1);
});

test('an older Yap’s save dropped an answer’s files: chat_files.json put them back', async ({ main }) => {
  await openConversation(main, 'Bed notes');
  await expect(card(main, 'notes.md')).toBeVisible();
  await expect(main.getByTestId('chat-share-caption')).toHaveText('Notes for the beds');
});

test('fileChip.js: badges and their families, a name cut in the middle, sizes, one chip a file a turn, a report’s line as links', () => {
  expect(['report.md', 'plan.docx', 'costs.xlsx', 'beds.csv', 'deck.pptx', 'map.svg', 'page.html', 'old.markdown', 'Makefile'].map((name) => badgeOf({ name }))).toEqual([
    'MD',
    'DOCX',
    'XLSX',
    'CSV',
    'PPTX',
    'SVG',
    'HTML',
    'MD',
    'TXT',
  ]);
  expect(badgeOf({ name: 'data.geojson', kind: 'code' })).toBe('CODE');
  expect(badgeOf({ name: 'notes', kind: 'file' })).toBe('FILE');
  expect(['a.md', 'a.pdf', 'a.csv', 'a.key', 'a.png', 'a.rs', 'a.7z', 'a.mp3', 'a.txt', 'a'].map((name) => familyOf({ name }))).toEqual([
    'doc',
    'pdf',
    'sheet',
    'slides',
    'picture',
    'code',
    'archive',
    'media',
    'text',
    'text',
  ]);
  expect(familyOf({ name: 'notes', kind: 'markdown' })).toBe('doc');
  // Cut in the middle: the ending and the word before it always whole; a short name never cut.
  expect(nameParts('yap-bug-report-code-chat-turns.md')).toEqual({ head: 'yap-bug-report-code-', tail: 'chat-turns.md' });
  expect(nameParts('north-border-bed-measurements.csv').tail).toBe('measurements.csv');
  expect(nameParts('notes.md')).toEqual({ head: '', tail: 'notes.md' });
  expect(nameParts('a-very-long-name.spreadsheet').tail).toBe('name.spreadsheet');
  expect([0, 572, 2048, 18_400, 1_500_000, 3_000_000_000].map(fileSize)).toEqual(['0B', '572B', '2.0 KB', '18 KB', '1.4 MB', '2.8 GB']);
  const B = String.fromCharCode(92); // a backslash
  const win = (p) => p.replaceAll('/', B);
  expect(normPath(' "C:/Garden/Plan.MD/" ')).toBe(win('c:/garden/plan.md'));
  // One chip a file a turn: the answer's share wins; an earlier report's file isn't shown again; a new turn starts afresh.
  const item = (p) => ({ name: p.split(/[\\/]/).pop(), path: p });
  const report = (...paths) => ({ role: 'user', kind: 'agentReport', text: 'done', files: [{ items: paths.map((p) => item(win(p))) }] });
  const messages = [
    { role: 'user', text: 'Write the plan' },
    report('C:/g/plan.md', 'C:/g/beds.csv'),
    report('C:/g/beds.csv', 'C:/g/water.md'),
    { role: 'assistant', text: 'Here.', files: [{ items: [item('c:/G/Plan.md')] }] },
    { role: 'user', text: 'Again' },
    report('C:/g/plan.md'),
    { role: 'assistant', text: 'Done.', liveFiles: [{ items: [item(win('C:/g/water.md'))] }] },
  ];
  const away = chippedElsewhere(messages);
  expect([...away.keys()]).toEqual([1, 2]);
  expect([...away.get(1)]).toEqual([win('c:/g/plan.md')]);
  expect([...away.get(2)]).toEqual([win('c:/g/beds.csv')]);
  // Each file once in a message's shares; a file with no path (a web picture) always itself.
  const seen = new Set();
  expect(onceEach([item(win('C:/a.md')), item('c:/A.md'), { name: 'web.png' }, { name: 'web.png' }], { seen }).map((i) => i.name)).toEqual(['a.md', 'web.png', 'web.png']);
  expect(onceEach([item(win('C:/a.md')), item(win('C:/b.md'))], { seen, skip: new Set([win('c:/b.md')]) })).toEqual([]);
  // "Changed …" as links: a name, a path in a folder, a full path; "and N more" stays words; one it doesn't name after it.
  const parts = changedParts(win('notes.md, data/beds.csv, D:/Out/report.md and 3 more'), [item(win('C:/g/notes.md')), item(win('C:/g/data/beds.csv')), item(win('D:/Out/report.md'))], [item(win('C:/g/extra.md'))]);
  expect(parts.map((p) => [p.text, !!p.item])).toEqual([
    ['notes.md', true],
    [', ', false],
    [win('data/beds.csv'), true],
    [', ', false],
    [win('D:/Out/report.md'), true],
    [' and 3 more', false],
    [', ', false],
    ['extra.md', true],
  ]);
  expect(changedParts('', [], [])).toEqual([]);
});
