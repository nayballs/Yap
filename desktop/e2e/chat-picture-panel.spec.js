// Pictures Chat found online, shown beside the chat (find and show,
// 2026-10-31: desktop/src/lib/views/chat/DocPanel.svelte, PictureTab.svelte,
// PicturePills.svelte, chatDoc.svelte.js, panelTabs.js, pictureZoom.js;
// desktop/src-tauri/src/chat/pictures.rs).
// - An answer's `pictures` show as thumbnail pills under it, beside its
//   document's pill; a click opens the panel on that picture.
// - The panel is a workspace: a tab per document and picture of the
//   conversation, newest last; a click shows one (the document keeps its
//   page), × closes one until its pill opens it again; the tabs come back
//   with the conversation.
// - A picture tab: fitted to the panel at its device pixels (Rust's
//   thumbnail, snip/thumbs.rs), zoomed with + / −, the wheel (around the
//   pointer), a double-click (fit ↔ 1:1) and the keys + / − / 0; dragged
//   about when zoomed; its caption and "From <page> · Open in <browser>" (the
//   browser, open_external); ⋯ Copy image / Save image as… (the chat
//   screenshots' own) / Open original; expand and back; light and dark.
// - The live `{kind: "picture"}` event opens the panel on its picture mid-
//   turn: faked here on the Voice Assistant's global stream event
//   (`yap-assistant-stream`), as the Rust tools that send it aren't needed
//   to test the page.
// Seeded: a conversation whose answer carries two pictures and a document
// (chats.json, notes.json) and the two PNGs in chat-images/.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig, closeToasts } from './support/fixtures.js';
import { encode } from './support/sharpness.js';

const NOW = Math.floor(Date.now() / 1000);
const MAP_ID = 'img_0a1b2c3d4e5f6071';
const BEACH_ID = 'img_9f8e7d6c5b4a3921';
const PAGE = { title: 'Fort Joy - Example Wiki', site: 'wiki.example.com', url: 'https://wiki.example.com/Fort_Joy' };
const MAP = {
  id: MAP_ID,
  w: 1600,
  h: 1200,
  caption: 'Fort Joy, every treasure spot marked',
  alt: 'Map of Fort Joy with red treasure markers',
  imageUrl: 'https://wiki.example.com/images/fort-joy-map.png',
  page: PAGE,
  n: 1,
};
const BEACH = {
  id: BEACH_ID,
  w: 900,
  h: 600,
  caption: 'The beach below the fort',
  alt: '',
  imageUrl: 'https://wiki.example.com/images/fort-joy-beach.png',
  page: { title: 'Fort Joy beach - Example Wiki', site: 'wiki.example.com', url: 'https://wiki.example.com/Fort_Joy_Beach' },
  n: 0,
};
const DOC = {
  id: 801,
  title: 'Fort Joy route',
  folder: 'Chat documents',
  source: 'chat',
  createdTs: NOW - 900,
  updatedTs: NOW - 900,
  content: '# Fort Joy route\n\nA short way round the fort, the treasure first.\n\n## The beach\n\nStart on the sand and keep the sea on your left.\n\n## The gate\n\nThe chest is behind the second tower.',
};
const TITLE = 'Fort Joy map';
const LIVE_TITLE = 'Beach question';
const CHATS = {
  conversations: [
    {
      id: 41,
      title: TITLE,
      createdTs: NOW - 900,
      updatedTs: NOW - 880,
      messages: [
        { role: 'user', text: 'Find me a map of Fort Joy with the treasure spots', ts: NOW - 900 },
        {
          role: 'assistant',
          text: 'Here is the map of Fort Joy from the wiki, every treasure spot marked [1]. The beach close-up is beside it, and the route is in a document.',
          ts: NOW - 890,
          sources: [{ n: 1, title: PAGE.title, url: PAGE.url, site: PAGE.site }],
          documents: [{ id: DOC.id, title: DOC.title }],
          pictures: [MAP, BEACH],
        },
      ],
    },
    {
      id: 42,
      title: LIVE_TITLE,
      createdTs: NOW - 300,
      updatedTs: NOW - 300,
      messages: [{ role: 'user', text: 'Show me the beach below the fort', ts: NOW - 300 }],
    },
  ],
};

/** A made-up map: parchment, a grid, a river and red treasure spots. */
function mapPicture(width, height) {
  const data = Buffer.alloc(width * height * 4);
  const spots = [
    [320, 260],
    [980, 410],
    [1210, 880],
    [540, 930],
  ];
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      let c = [233, 220, 184];
      if (x % 100 === 0 || y % 100 === 0) c = [201, 186, 148];
      const river = Math.abs(y - (0.45 * x + 180 + 40 * Math.sin(x / 90)));
      if (river < 22) c = [92, 142, 186];
      for (const [sx, sy] of spots) if ((x - sx) ** 2 + (y - sy) ** 2 < 18 ** 2) c = [196, 48, 40];
      data.set([...c, 255], (y * width + x) * 4);
    }
  }
  return { width, height, data };
}

/** A made-up beach: sea over sand, a line of foam. */
function beachPicture(width, height) {
  const data = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const shore = height * 0.45 + 30 * Math.sin(x / 70);
      const c = y < shore - 6 ? [64, 132, 176] : y < shore + 6 ? [236, 240, 238] : [222, 196, 140];
      data.set([...c, 255], (y * width + x) * 4);
    }
  }
  return { width, height, data };
}

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'chat-picture-panel',
        data: { 'chats.json': CHATS, 'notes.json': { folders: ['Chat documents'], notes: [DOC] } },
        config: { meetingReminder: 'never' },
        prepare: (dir) => {
          const images = path.join(dir, 'chat-images');
          fs.mkdirSync(images, { recursive: true });
          fs.writeFileSync(path.join(images, `${MAP_ID}.png`), encode(mapPicture(MAP.w, MAP.h)));
          fs.writeFileSync(path.join(images, `${BEACH_ID}.png`), encode(beachPicture(BEACH.w, BEACH.h)));
        },
      });
    },
    { scope: 'worker' },
  ],
});

const panel = (main) => main.getByTestId('chat-doc');
const picture = (main) => main.getByTestId('chat-picture');
const stage = (main) => main.getByTestId('chat-picture-stage');
const zoom = (main) => main.getByTestId('chat-picture-zoom');
const tabs = (main) => main.getByTestId('chat-panel-tab');
const pills = (main) => main.getByTestId('chat-picture-pill');
const fullImg = (main) => main.getByTestId('chat-picture-img');
const thumb = (main) => main.getByTestId('chat-picture-thumb');

/** The conversation `title` open (the list folded away for an open panel: brought back). */
async function openConversation(main, title) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  await main.locator('.conv').filter({ hasText: title }).click();
}

/** An <img>'s box on screen (CSS px), natural size, address and the pixel ratio. */
function placement(img) {
  return img.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height, nw: el.naturalWidth, nh: el.naturalHeight, src: el.getAttribute('src'), dpr: window.devicePixelRatio, opacity: getComputedStyle(el).opacity };
  });
}

async function loaded(img) {
  await expect.poll(() => img.evaluate((el) => el.complete && el.naturalWidth > 0), { message: 'the picture to load' }).toBe(true);
}

/** The whole picture's CSS width once its glide has settled. */
async function settledWidth(main) {
  await main.waitForTimeout(260);
  return (await placement(fullImg(main))).w;
}

test('pictures under an answer open beside the chat: tabs, zoom and pan, the source, Copy / Save, dark', async ({ yap, main, shot }) => {
  await openConversation(main, TITLE);

  // The pills: the document's and a thumbnail for each picture, one row.
  await expect(main.getByTestId('chat-doc-pill')).toHaveCount(1);
  await expect(pills(main)).toHaveCount(2);
  await expect(pills(main).nth(0)).toContainText(MAP.caption);
  await expect(pills(main).nth(0)).toContainText('wiki.example.com');
  await expect(pills(main).nth(1)).toContainText(BEACH.caption);
  await expect(main.getByRole('group', { name: 'Documents and pictures' })).toBeVisible();
  // Each pill's thumbnail at its device pixels (snip/thumbs.rs).
  const pillImg = pills(main).nth(0).locator('img');
  await loaded(pillImg);
  const pa = await placement(pillImg);
  expect(pa.src).toMatch(new RegExp(`${MAP_ID}\\?w=\\d+&h=\\d+`));
  expect(Math.abs(pa.w * pa.dpr - pa.nw)).toBeLessThan(0.6);
  await expect(panel(main)).toHaveCount(0);
  await shot(main, '00-pills-under-the-answer');

  // A click opens the panel on that picture, with a tab for everything in the chat.
  await pills(main).nth(0).click();
  await expect(picture(main)).toBeVisible();
  await expect(picture(main).locator('.pictitle')).toHaveText(MAP.caption);
  await expect(main.getByTestId('chat-picture-caption')).toHaveText(MAP.caption);
  await expect(main.getByTestId('chat-picture-source')).toHaveText(/From Fort Joy - Example Wiki\s*·\s*Open in your browser/);
  await expect(tabs(main)).toHaveText([DOC.title, MAP.caption, BEACH.caption]);
  await expect(tabs(main).nth(1)).toHaveAttribute('aria-selected', 'true');

  // Fitted: Rust's thumbnail at exactly its device pixels, the label "Fit".
  await expect(zoom(main)).toHaveText('Fit');
  await loaded(thumb(main));
  await expect.poll(async () => (await placement(thumb(main))).opacity).toBe('1');
  const fit = await placement(thumb(main));
  expect(fit.src).toMatch(/\?w=\d+&h=\d+/);
  expect(Math.abs(fit.w * fit.dpr - fit.nw), `${fit.w} CSS px × ${fit.dpr} vs ${fit.nw} px`).toBeLessThan(0.6);
  expect(Math.abs(fit.h * fit.dpr - fit.nh)).toBeLessThan(0.6);
  // The whole picture fits the stage.
  const st = await stage(main).boundingBox();
  expect(fit.w).toBeLessThanOrEqual(st.width);
  expect(fit.h).toBeLessThanOrEqual(st.height);
  await shot(main, '01-picture-tab-light');

  // + zooms in, − back out to the fit.
  const fitW = (await placement(fullImg(main))).w;
  await picture(main).getByRole('button', { name: 'Zoom in' }).click();
  await expect(zoom(main)).not.toHaveText('Fit');
  expect(await settledWidth(main)).toBeGreaterThan(fitW * 1.2);
  await expect(thumb(main)).not.toHaveClass(/\bon\b/);
  await picture(main).getByRole('button', { name: 'Zoom out' }).click();
  await expect(zoom(main)).toHaveText('Fit');
  await expect(picture(main).getByRole('button', { name: 'Zoom out' })).toBeDisabled();

  // The wheel zooms around the pointer: the picture's point under it stays put.
  await loaded(fullImg(main));
  const px = st.x + st.width * 0.3;
  const py = st.y + st.height * 0.4;
  expect(await settledWidth(main)).toBeCloseTo(fitW, 0);
  // (Measured once it's past the stage both ways: at its edge it stops,
  // never leaving the stage empty past it.)
  await main.mouse.move(px, py);
  await main.mouse.wheel(0, -300);
  await expect(zoom(main)).not.toHaveText('Fit');
  await expect.poll(async () => (await placement(fullImg(main))).h).toBeGreaterThan(st.height);
  const before = await placement(fullImg(main));
  const u = (px - before.x) / before.w;
  const v = (py - before.y) / before.h;
  await main.mouse.wheel(0, -300);
  await expect.poll(async () => (await placement(fullImg(main))).w).toBeGreaterThan(before.w * 1.5);
  const after = await placement(fullImg(main));
  expect(Math.abs(after.x + u * after.w - px)).toBeLessThan(2);
  expect(Math.abs(after.y + v * after.h - py)).toBeLessThan(2);

  // Zoomed past the panel: dragged about, never off its edge.
  await main.mouse.move(st.x + st.width / 2, st.y + st.height / 2);
  await main.mouse.down();
  await main.mouse.move(st.x + st.width / 2 + 60, st.y + st.height / 2 + 40, { steps: 4 });
  await main.mouse.up();
  const dragged = await placement(fullImg(main));
  expect(dragged.x).not.toBe(after.x);
  expect(dragged.x).toBeLessThanOrEqual(st.x + 0.5);
  expect(dragged.y).toBeLessThanOrEqual(st.y + 0.5);
  await shot(main, '02-picture-zoomed');

  // A double-click: back to the fit; again, 1:1 (one image pixel per device pixel).
  await stage(main).dblclick();
  await expect(zoom(main)).toHaveText('Fit');
  await stage(main).dblclick({ position: { x: st.width / 2, y: st.height / 2 } });
  await expect(zoom(main)).toHaveText('100%');
  const one = await settledWidth(main);
  const dpr = await main.evaluate(() => window.devicePixelRatio);
  expect(Math.abs(one * dpr - MAP.w)).toBeLessThan(1);

  // The keys: 0 fits it, + and − step.
  await stage(main).focus();
  await main.keyboard.press('0');
  await expect(zoom(main)).toHaveText('Fit');
  await main.keyboard.press('+');
  await expect(zoom(main)).not.toHaveText('Fit');
  await main.keyboard.press('-');
  await expect(zoom(main)).toHaveText('Fit');

  // Open in your browser: the person's browser (a test run keeps the link instead).
  await main.getByTestId('chat-picture-source').getByRole('button', { name: 'Open in your browser' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain(PAGE.url);

  // ⋯: Copy image (the chat screenshots' own copy), Save image as…, Open original.
  const file = path.join(yap.dataDir, 'chat-images', `${MAP_ID}.png`);
  await picture(main).getByRole('button', { name: 'More for the picture' }).click();
  await expect(main.getByRole('menuitem', { name: 'Save image as…' })).toBeVisible();
  await expect(main.getByRole('menuitem', { name: 'Open original in your browser' })).toBeVisible();
  await main.getByRole('menuitem', { name: 'Copy image' }).click();
  await expect.poll(() => yap.invoke('e2e_snip_clipboard')).toBe(fs.readFileSync(file).toString('base64'));
  await expect(main.getByRole('status').filter({ hasText: 'Picture copied' })).toBeVisible();
  const saved = path.join(yap.dataDir, 'saved-pictures', 'fort-joy.png');
  fs.mkdirSync(path.dirname(saved), { recursive: true });
  await yap.invoke('e2e_snip_save_path', { path: saved });
  await picture(main).getByRole('button', { name: 'More for the picture' }).click();
  await main.getByRole('menuitem', { name: 'Save image as…' }).click();
  await expect.poll(() => fs.existsSync(saved)).toBe(true);
  expect(fs.readFileSync(saved).equals(fs.readFileSync(file))).toBe(true);
  await stage(main).click({ button: 'right' });
  await main.getByRole('menuitem', { name: 'Open original in your browser' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain(MAP.imageUrl);

  // Zoomed in, then the document's tab: its page as ever. Back: the zoom kept.
  await picture(main).getByRole('button', { name: 'Zoom in' }).click();
  const kept = await zoom(main).textContent();
  await tabs(main).filter({ hasText: DOC.title }).click();
  await expect(picture(main)).toHaveCount(0);
  await expect(panel(main).locator('.doctitle')).toHaveText(DOC.title);
  await expect(panel(main).locator('.docbody h3')).toHaveText(['The beach', 'The gate']);
  await expect(tabs(main).nth(0)).toHaveAttribute('aria-selected', 'true');
  await panel(main).getByRole('button', { name: 'Source' }).click();
  await expect(panel(main).getByRole('textbox', { name: 'Document source (Markdown)' })).toHaveValue(DOC.content);
  await tabs(main).filter({ hasText: MAP.caption }).click();
  await expect(zoom(main)).toHaveText(kept);
  // ← / → on the row move between tabs.
  await tabs(main).nth(1).focus();
  await main.keyboard.press('ArrowRight');
  await expect(picture(main).locator('.pictitle')).toHaveText(BEACH.caption);
  await expect(main.getByTestId('chat-picture-source')).toHaveText(/From Fort Joy beach - Example Wiki/);
  await main.keyboard.press('ArrowLeft');
  await main.keyboard.press('ArrowLeft');
  // The document again: still in Source, as it was left.
  await expect(panel(main).getByRole('textbox', { name: 'Document source (Markdown)' })).toBeVisible();
  await panel(main).getByRole('button', { name: 'Preview' }).click();

  // × closes a tab until its pill opens it again.
  await tabs(main).filter({ hasText: BEACH.caption }).hover();
  await main.getByRole('button', { name: `Close ${BEACH.caption}` }).click();
  await expect(tabs(main)).toHaveText([DOC.title, MAP.caption]);
  await pills(main).nth(1).click();
  await expect(tabs(main)).toHaveText([DOC.title, MAP.caption, BEACH.caption]);
  await expect(tabs(main).nth(2)).toHaveAttribute('aria-selected', 'true');
  // The one showing closed: its neighbour shows.
  await main.getByRole('button', { name: `Close ${BEACH.caption}` }).click();
  await expect(picture(main).locator('.pictitle')).toHaveText(MAP.caption);

  // Expanded over the chat: fitted to the bigger room. Back beside the chat.
  await expect(zoom(main)).toHaveText(kept);
  await stage(main).dblclick();
  await expect(zoom(main)).toHaveText('Fit');
  const small = await settledWidth(main);
  await picture(main).getByRole('button', { name: 'Expand to full width' }).click();
  await expect(panel(main)).toHaveClass(/\bcover\b/);
  await expect.poll(async () => (await placement(fullImg(main))).w).toBeGreaterThan(small + 50);
  await picture(main).getByRole('button', { name: 'Back beside the chat' }).click();
  await expect(panel(main)).not.toHaveClass(/\bcover\b/);

  // **A tab closed stays closed** (kept with the conversation: chats.rs `closed_tabs`) — before, every
  // picture, page and document its answers had shown came back each time the chat was opened.
  await expect.poll(async () => (await yap.invoke('chat_get', { id: 41 })).closedTabs ?? []).toEqual([`pic:${BEACH_ID}`]);
  // Another chat: the panel goes. Back: the pills, and the tabs with the panel — less the one closed.
  await main.getByRole('button', { name: 'Show your chats' }).click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(panel(main)).toHaveCount(0);
  await main.locator('.conv').filter({ hasText: TITLE }).click();
  await expect(pills(main)).toHaveCount(2);
  await expect(panel(main)).toHaveCount(0);
  await main.getByTestId('chat-doc-pill').click();
  await expect(tabs(main)).toHaveText([DOC.title, MAP.caption]);
  await expect(panel(main).locator('.doctitle')).toHaveText(DOC.title);
  await shot(main, '02b-a-closed-tab-stays-closed');
  // Its pill opens it again, and it's off the list: it comes back with the chat from now on.
  await pills(main).nth(1).click();
  await expect(tabs(main)).toHaveText([DOC.title, MAP.caption, BEACH.caption]);
  await expect.poll(async () => (await yap.invoke('chat_get', { id: 41 })).closedTabs ?? []).toEqual([]);
  await tabs(main).filter({ hasText: DOC.title }).click();
  await expect(panel(main).locator('.doctitle')).toHaveText(DOC.title);
  await tabs(main).filter({ hasText: MAP.caption }).click();
  await expect(picture(main)).toBeVisible();

  // Dark, as it looks there.
  await stage(main).dblclick();
  if ((await zoom(main).textContent()) !== 'Fit') await stage(main).dblclick();
  await expect(zoom(main)).toHaveText('Fit');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await closeToasts(main);
  await expect.poll(async () => (await placement(thumb(main))).opacity).toBe('1');
  // The stage's ground is the dark paper's, not the light one's.
  const ground = await stage(main).evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(ground).toBe('rgb(38, 35, 30)');
  await shot(main, '03-picture-tab-dark');
  await setConfig(yap, main, { theme: 'light' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('light');
  await picture(main).getByRole('button', { name: 'Close the panel' }).click();
  await expect(panel(main)).toHaveCount(0);
});

test('a picture shown mid-turn opens the panel on it (the live picture event)', async ({ yap, main, shot }) => {
  // The panel open on another conversation's pictures.
  await openConversation(main, TITLE);
  await pills(main).nth(0).click();
  await expect(tabs(main)).toHaveCount(3);
  const turn = 'chat:42:0';
  // A question asked out loud lands in its own conversation (chat/assistant.rs):
  // the other one's panel goes with it…
  await yap.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: { phase: 'asked', conversationId: 42, userMessageId: turn } });
  await expect(main.locator('.cbubble.user').last()).toContainText('Show me the beach below the fort');
  await expect(panel(main)).toHaveCount(0);
  // …and its turn shows a picture (`show_picture`'s event): the panel opens on it.
  const live = { ...BEACH, caption: 'The beach, from the wiki' };
  await yap.invoke('plugin:event|emit', { event: 'yap-assistant-stream', payload: { conversationId: 42, userMessageId: turn, event: { kind: 'picture', picture: live } } });
  await expect(picture(main)).toBeVisible();
  await expect(picture(main).locator('.pictitle')).toHaveText(live.caption);
  await expect(main.getByTestId('chat-picture-source')).toHaveText(/From Fort Joy beach - Example Wiki/);
  await loaded(fullImg(main));
  // One thing in the panel: no row of tabs.
  await expect(tabs(main)).toHaveCount(0);
  await shot(main, '04-shown-mid-turn');
  // A picture from an image search names no page (Tavily's): no Open in your browser.
  const bare = { ...MAP, id: MAP_ID, caption: 'The fort from above', page: { title: 'images.example.com', site: 'images.example.com', url: '' }, n: 0 };
  await yap.invoke('plugin:event|emit', { event: 'yap-assistant-stream', payload: { conversationId: 42, userMessageId: turn, event: { kind: 'picture', picture: bare } } });
  await expect(picture(main).locator('.pictitle')).toHaveText(bare.caption);
  await expect(main.getByTestId('chat-picture-source')).toHaveText('From images.example.com');
  await expect(main.getByTestId('chat-picture-source').getByRole('button', { name: 'Open in your browser' })).toHaveCount(0);
  await expect(tabs(main)).toHaveText([live.caption, bare.caption]);
  await tabs(main).first().click();
  await expect(picture(main).locator('.pictitle')).toHaveText(live.caption);
  // The turn ends (no answer saved here): the panel stays on it.
  await yap.invoke('plugin:event|emit', { event: 'yap-assistant-turn', payload: { phase: 'failed', conversationId: 42, userMessageId: turn } });
  await expect(picture(main)).toBeVisible();
  await picture(main).getByRole('button', { name: 'Close the panel' }).click();
  await expect(panel(main)).toHaveCount(0);
});
