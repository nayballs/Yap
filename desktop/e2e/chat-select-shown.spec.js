// Picking chats acts only on the rows the list shows (ConversationList.svelte
// `rows`, chatState.svelte.js `shownPicked`, `askDeleteMany`): with Yap Code
// on, Select all, Ctrl+A and "Delete all chats…" are the side's own (Chat or
// Code), never a folded workspace's, an archived chat or a meeting's Ask
// thread (chat/note_chats.rs; neither is ever in the list); a selection lets
// go across the Chat | Code switch and while the search's results or Archived
// show in the list's place (no ⋯ there either). And the "N selected" bar
// fits the list on one row at every width its edge allows (its ✕ is Cancel;
// with no room for "selected", the rows' ✓ and the number, still read out as
// "N selected"), above the edge, the edge still dragged above and below it.
// Fictional data only, the workspace in this run's own folder.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView, setConfig } from './support/fixtures.js';
import { RUNS } from './support/yap.js';
import { toSide } from './support/code-chats.js';

const NOW = Math.floor(Date.now() / 1000);
const GARDEN = path.join(RUNS, 'chat-select-shown-spec', 'folders', 'garden-planner');
const conv = (id, title, ago, extra = {}) => ({
  id,
  title,
  createdTs: NOW - ago,
  updatedTs: NOW - ago,
  messages: [
    { role: 'user', text: `${title} question`, ts: NOW - ago },
    { role: 'assistant', text: `${title} answer`, ts: NOW - ago + 1 },
  ],
  ...extra,
});
// The Chat side: Pea trellis, Seed tray. The Code side: Garden code and eleven
// more (attached below: 12 to pick, two digits in the bar). Never in the
// list: Bean diary (archived), the meeting's Ask thread.
const MORE_CODE = Array.from({ length: 11 }, (_, i) => conv(6 + i, `Garden task ${i + 1}`, 1000 + i));
const CODE_IDS = [3, ...MORE_CODE.map((c) => c.id)];
const CHATS = {
  conversations: [
    conv(1, 'Seed tray', 400),
    conv(2, 'Pea trellis', 300),
    conv(3, 'Garden code', 200),
    conv(4, 'Bean diary', 500, { archivedTs: NOW - 100 }),
    conv(5, 'Allotment meeting questions', 100, { note: 900 }),
    ...MORE_CODE,
  ],
};

test.use({ yapOptions: { name: 'chat-select-shown', data: { 'chats.json': CHATS } } });

const rowTitles = (main) => main.locator('.conv .ctitle');
const row = (main, title) => main.locator('.conv').filter({ hasText: title });
const bar = (main) => main.getByRole('toolbar', { name: 'Selected chats' });
const more = (main) => main.getByRole('button', { name: 'More for chats' });
const menu = (main) => main.getByRole('menu').first();
const dialog = (main) => main.getByRole('alertdialog');
const list = (main) => main.locator('#chat-list');
const edge = (main) => main.getByRole('separator', { name: 'Resize your chats' });
const ids = (yap) => yap.readJson('chats.json').conversations.map((c) => c.id).sort((a, b) => a - b);
// What the Chat side's "Delete all chats…" leaves: the Code chats, the archived one, the Ask thread.
const LEFT = [...CODE_IDS, 4, 5].sort((a, b) => a - b);

async function selectAll(main) {
  await more(main).click();
  await menu(main).getByRole('menuitem', { name: 'Select all' }).click();
}

/** Yap Code on, the twelve Code chats in garden-planner (once per Yap). */
async function codeOn(yap, main) {
  if (!fs.existsSync(GARDEN)) fs.mkdirSync(GARDEN, { recursive: true });
  await setConfig(yap, main, { agentsEnabled: true, theme: 'light' });
  const have = (await yap.invoke('workspaces_list')).map((w) => w.name);
  if (!have.includes('garden-planner')) await yap.invoke('workspaces_add', { path: GARDEN });
  for (const id of CODE_IDS) {
    if (!(await yap.invoke('code_chat_workspace_get', { conversation: id }))) {
      await yap.invoke('code_chat_workspace_set', { conversation: id, folder: GARDEN });
    }
  }
  await openView(main, 'Chat');
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** Where things sit: the list, its edge, the bar and its parts (CSS px). */
const layoutOf = (main) =>
  main.evaluate(() => {
    const r = (el) => {
      const b = el.getBoundingClientRect();
      return { left: b.left, right: b.right, top: b.top, bottom: b.bottom, w: b.width };
    };
    const bar = document.querySelector('.selbar');
    const count = bar.querySelector('.selcount');
    const x = bar.querySelector('.selx');
    const xb = x.getBoundingClientRect();
    const hit = document.elementFromPoint(xb.left + xb.width / 2, xb.top + xb.height / 2);
    return {
      list: r(document.querySelector('#chat-list')),
      edge: r(document.querySelector('.listedge')),
      bar: r(bar),
      fits: bar.scrollWidth <= bar.clientWidth,
      parts: [...bar.children].map(r),
      xIsHit: !!hit && x.contains(hit),
      // The count: how it fits, and what of it shows (on its one line, whole).
      fit: count.dataset.fit,
      // (On its one line: whatever wrapped is below the count's box.)
      seen: [...count.children]
        .filter((k) => getComputedStyle(k).display !== 'none' && k.getBoundingClientRect().top < count.getBoundingClientRect().bottom)
        .map((k) => ({ cls: k.className.split(' ')[0], text: k.textContent, right: k.getBoundingClientRect().right })),
      countRight: count.getBoundingClientRect().right,
    };
  });

test('Select all, Ctrl+A and Delete all chats… are one side’s shown chats; a selection never crosses the switch', async ({ yap, main }) => {
  await codeOn(yap, main);
  await toSide(main, 'Chat');
  await expect(rowTitles(main)).toHaveText(['Pea trellis', 'Seed tray']);
  await selectAll(main);
  await expect(bar(main)).toContainText('2 selected');

  // Over to Code: the Chat side's selection stays behind, and isn't picked again on the way back.
  await toSide(main, 'Code');
  await expect(rowTitles(main)).toHaveCount(12);
  await expect(rowTitles(main).first()).toHaveText('Garden code');
  await expect(bar(main)).toHaveCount(0);
  await expect(main.locator('.conv.picked')).toHaveCount(0);
  // Ctrl+A here: the Code chats alone.
  await row(main, 'Garden code').focus();
  await main.keyboard.press('Control+A');
  await expect(bar(main)).toContainText('12 selected');
  // Its workspace folded: nothing shown, so nothing picked and no ⋯.
  await main.getByTestId('chat-workspace-group').click();
  await expect(rowTitles(main)).toHaveCount(0);
  await expect(bar(main)).toHaveCount(0);
  await expect(more(main)).toHaveCount(0);
  await main.getByTestId('chat-workspace-group').click();
  await expect(rowTitles(main)).toHaveCount(12);
  await expect(bar(main)).toHaveCount(0);

  await toSide(main, 'Chat');
  await expect(bar(main)).toHaveCount(0);
  // Delete all chats… on the Chat side: its two, asked first with their messages.
  await more(main).click();
  await menu(main).getByRole('menuitem', { name: 'Delete all chats…' }).click();
  await expect(dialog(main)).toHaveAccessibleName('Delete all 2 chats?');
  await expect(dialog(main)).toContainText('Their 4 messages go for good. Your notes and meetings stay.');
  await dialog(main).getByRole('button', { name: 'Delete' }).click();
  await expect(main.getByText('Start your first conversation')).toBeVisible();
  // The Code chat, the archived one and the meeting's Ask thread all stay.
  await expect.poll(() => ids(yap)).toEqual(LEFT);
  await toSide(main, 'Code');
  await expect(rowTitles(main)).toHaveCount(12);
  await toSide(main, 'Chat');
});

test('while the search or Archived shows: no ⋯, the selection lets go, Ctrl+A picks nothing', async ({ yap, main }) => {
  await codeOn(yap, main);
  await toSide(main, 'Code');
  await row(main, 'Garden code').focus();
  await main.keyboard.press('Control+A');
  await expect(bar(main)).toContainText('12 selected');

  // The search's results in the list's place.
  const search = main.getByRole('searchbox', { name: 'Search chats' });
  await search.fill('diary');
  await expect(main.locator('.row.hit').first()).toContainText('Bean diary');
  await expect(bar(main)).toHaveCount(0);
  await expect(more(main)).toHaveCount(0);
  await main.locator('.row.hit').first().focus();
  await main.keyboard.press('Control+A');
  await expect(bar(main)).toHaveCount(0);
  await search.press('Escape');
  await expect(rowTitles(main)).toHaveCount(12);
  await expect(bar(main)).toHaveCount(0);
  await expect(more(main)).toBeVisible();

  // Archived in the list's place: "Delete all chats…" is never offered over it.
  await main.getByRole('button', { name: /^Archived/ }).click();
  await expect(main.getByRole('region', { name: 'Archived chats' })).toContainText('Bean diary');
  await expect(more(main)).toHaveCount(0);
  await main.getByRole('region', { name: 'Archived chats' }).locator('.row').first().focus();
  await main.keyboard.press('Control+A');
  await expect(bar(main)).toHaveCount(0);
  await main.getByRole('region', { name: 'Archived chats' }).locator('.back').click();
  await expect(rowTitles(main)).toHaveCount(12);
  expect(ids(yap)).toEqual(LEFT);
});

test('the bar fits the list on one row at 224 and 200 px, its count giving way, above its edge; the edge still drags above and below it', async ({ yap, main, shot }) => {
  await codeOn(yap, main);
  await toSide(main, 'Code');
  const pickOne = async () => {
    // The open chat Ctrl+clicked: it alone.
    await row(main, 'Garden code').focus();
    await main.keyboard.press('Escape');
    await row(main, 'Garden code').click({ modifiers: ['Control'] });
    await expect(bar(main)).toContainText('1 selected');
  };
  const pickAll = async () => {
    await row(main, 'Garden code').focus();
    await main.keyboard.press('Control+A');
    await expect(bar(main)).toContainText('12 selected');
  };
  await row(main, 'Garden code').click();
  await pickOne();
  await expect(bar(main).getByRole('button')).toHaveText(['Archive', 'Delete', '']);
  const cancel = bar(main).getByRole('button', { name: 'Cancel' });
  await expect(cancel).toHaveAttribute('title', 'Cancel · Esc');

  let k = 0;
  /** The bar as it stands: one row inside the list's border, the ✕ its own, the count whole. */
  const check = async (width, n) => {
    await expect(edge(main)).toHaveAttribute('aria-valuenow', String(width));
    // Read out as "N selected" however it shows.
    await expect(bar(main)).toContainText(`${n} selected`);
    const l = await layoutOf(main);
    expect(l.fits).toBe(true);
    for (const p of l.parts) expect(p.right).toBeLessThanOrEqual(l.list.right - 1);
    expect(l.bar.bottom - l.bar.top).toBeLessThanOrEqual(36);
    expect(l.xIsHit).toBe(true);
    // What shows of the count is whole: the number always, "selected" only
    // when it fits, else the rows' ✓ beside the number.
    for (const p of l.seen) expect(p.right).toBeLessThanOrEqual(l.countRight + 0.5);
    const seen = l.seen.map((p) => p.cls);
    expect(l.seen.find((p) => p.cls === 'selnum')?.text).toBe(String(n));
    expect(seen.includes('selword')).toBe(l.fit === 'full');
    expect(seen.includes('pickmark')).toBe(l.fit === 'short');
    const name = `${width}px-${n}-selected`;
    await shot(main, `${String(++k).padStart(2, '0')}-bar-${name}-light`);
    await theme(yap, main, 'dark');
    await shot(main, `${String(++k).padStart(2, '0')}-bar-${name}-dark`);
    await theme(yap, main, 'light');
    return l.fit;
  };

  // 224 px (the list's own width): "1 selected" in full.
  expect(await check(224, 1)).toBe('full');
  await pickAll();
  await check(224, 12);
  // 200 px (the narrowest): no room for "selected" — the ✓ and the number.
  await edge(main).focus();
  await main.keyboard.press('Home');
  expect(await check(200, 12)).not.toBe('full');
  await pickOne();
  expect(await check(200, 1)).toBe('short');

  // The ✕ is Cancel.
  await cancel.click();
  await expect(bar(main)).toHaveCount(0);
  await pickOne();

  // The edge dragged above the bar (the list's head) and below it (a row), the selection up.
  const drag = async (y, by) => {
    const e = await edge(main).boundingBox();
    const x = e.x + e.width / 2;
    await main.mouse.move(x, y);
    await main.mouse.down();
    await main.mouse.move(x + by / 2, y, { steps: 3 });
    await main.mouse.move(x + by, y, { steps: 3 });
    await main.mouse.up();
  };
  const b = (await layoutOf(main)).bar;
  const head = await main.locator('#chat-list .head').boundingBox();
  const below = await row(main, 'Garden code').boundingBox();
  expect(head.y + head.height / 2).toBeLessThan(b.top);
  expect(below.y + below.height / 2).toBeGreaterThan(b.bottom);
  await drag(head.y + head.height / 2, 40);
  await expect(edge(main)).toHaveAttribute('aria-valuenow', '240');
  // Wider: room for "selected" again.
  await expect(main.locator('.selcount')).toHaveAttribute('data-fit', 'full');
  await drag(below.y + below.height / 2, -40);
  await expect(edge(main)).toHaveAttribute('aria-valuenow', '200');
  await expect(main.locator('.selcount')).toHaveAttribute('data-fit', 'short');
  await expect(bar(main)).toContainText('1 selected');
  // Back to 224 for whatever runs next; Esc lets the selection go.
  await edge(main).dblclick();
  await expect(edge(main)).toHaveAttribute('aria-valuenow', '224');
  await row(main, 'Garden code').focus();
  await main.keyboard.press('Escape');
  await expect(bar(main)).toHaveCount(0);
  expect(await list(main).evaluate((el) => Math.round(el.getBoundingClientRect().width))).toBe(224);
  expect(ids(yap)).toEqual(LEFT);
});
