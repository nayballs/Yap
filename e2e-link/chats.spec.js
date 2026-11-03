// The drawer and the phone's own conversation (the Claude app's layout,
// 2026-11-03): ☰ lists Chat's conversations on the PC (`chats`), a search
// filters them, one tapped is opened (`open_chat` → `thread`) and the
// phone's questions go there (`chat` on `ask` / `start`), + New chat starts
// afresh (`new_chat`), the PC says where a question went (`chat`) and a
// rename (`title`); a reload asks for the conversation again; a PC from
// before has no ☰. Yap on the PC mocked.
import { test, expect } from '@playwright/test';
import {
  servePage,
  seedDevice,
  mockPc,
  open,
  ready,
  ask,
  type,
  snapshot,
  RECENTS,
  CAN_BEFORE_CHATS,
  PC,
} from './support/harness.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

const drawer = (page) => page.getByTestId('drawer');
const recents = (page) => page.getByTestId('recent');

test('☰ opens the drawer: Yap, a search, Recents from the PC, the PC and + New chat', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('title')).toHaveText(`Yap · ${PC}`);
  await page.getByTestId('menu').click();
  await expect(drawer(page)).toBeVisible();
  await expect.poll(() => pc.of('chats').length).toBe(1);
  await expect(drawer(page).getByRole('heading', { name: 'Yap' })).toBeVisible();
  await expect(recents(page)).toHaveText(RECENTS.map((r) => r.title));
  await expect(page.getByTestId('drawer-pc')).toHaveText(PC);
  await expect(page.getByTestId('new-chat')).toHaveText('New chat');
  // Nothing marked yet: this phone is in no conversation of its own.
  await expect(drawer(page).locator('[aria-current="true"]')).toHaveCount(0);
  // The search filters the titles (every word, any case, without accents).
  await page.getByTestId('drawer-search').fill('SOUR sched');
  await expect(recents(page)).toHaveText(['Sourdough starter schedule']);
  await page.getByTestId('drawer-search').fill('kayak');
  await expect(recents(page)).toHaveCount(0);
  await expect(page.getByTestId('recents-empty')).toHaveText('No chats match that.');
  await page.getByTestId('drawer-search').fill('');
  await expect(recents(page)).toHaveCount(RECENTS.length);
  // Esc closes it.
  await page.keyboard.press('Escape');
  await expect(drawer(page)).toBeHidden();
});

test('a chat opened from the drawer: its messages, its title, and the next questions go there', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await recents(page).filter({ hasText: 'Trip packing list' }).click();
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => pc.of('open_chat')).toEqual([{ type: 'open_chat', id: 41 }]);
  // Its messages, as Chat has them: the questions in bubbles (🎙 when said,
  // its picture counted), the answers as text with Copy and ▶.
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(page.getByTestId('heard').first()).toHaveText('What should I pack for three days of hiking in the rain?');
  await expect(page.getByTestId('answer').first().locator('li')).toHaveCount(3);
  await expect(page.getByTestId('turn').last().getByTestId('turn-pictures')).toHaveText('1 picture');
  await expect(page.getByRole('img', { name: 'Asked by voice' })).toHaveCount(1);
  await expect(page.getByTestId('read-aloud')).toHaveCount(2);
  await expect(page.getByTestId('title-sub')).toHaveText('Trip packing list');
  expect((await snapshot(page)).chat).toBe(41);
  // ▶ on an old answer reads it by its id.
  await page.getByTestId('read-aloud').first().click();
  await expect.poll(() => pc.of('read_aloud')).toEqual([{ type: 'read_aloud', id: 'chat:41:1' }]);
  // Typed and spoken questions go to it.
  await type(page, pc, 'And for five days?');
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'And for five days?', chat: 41 });
  await ask(page, pc, 200);
  expect(pc.of('start').at(-1)).toEqual({ type: 'start', mode: 'ask', chat: 41 });
  // The drawer marks it.
  await page.getByTestId('menu').click();
  await expect(drawer(page).locator('[aria-current="true"]')).toHaveText('Trip packing list');
});

test('+ New chat: the thread empties, the next question starts one, and the PC says which', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await recents(page).filter({ hasText: 'Sourdough' }).click();
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await page.getByTestId('menu').click();
  await page.getByTestId('new-chat').click();
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => pc.of('new_chat').length).toBe(1);
  await expect(page.getByTestId('turn')).toHaveCount(0);
  await expect(page.getByText('Ask Yap anything')).toBeVisible();
  await expect(page.getByTestId('title-sub')).toBeHidden();
  await type(page, pc, 'Ideas for a rainy Saturday?');
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Ideas for a rainy Saturday?', chat: 'new' });
  // The PC made the conversation: its id and title; the next one goes there.
  pc.send({ type: 'heard', text: 'Ideas for a rainy Saturday?' });
  pc.send({ type: 'chat', id: 52, title: 'Ideas for a rainy Saturday?' });
  pc.send({ type: 'answer', text: 'A museum, a film, or a board game afternoon.', final: true, id: 'chat:52:1' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('title-sub')).toHaveText('Ideas for a rainy Saturday?');
  await type(page, pc, 'Which museum?');
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Which museum?', chat: 52 });
  // Renamed in Chat on the PC: the header and the drawer follow.
  pc.send({ type: 'title', id: 52, title: 'Rainy day plans' });
  await expect(page.getByTestId('title-sub')).toHaveText('Rainy day plans');
  // The PC's list (the mock's) doesn't have it; the drawer adds it on top.
  pc.recents = [{ id: 52, title: 'Rainy day plans', ts: 1_793_800_000 }, ...RECENTS];
  await page.getByTestId('menu').click();
  await expect(recents(page).first()).toHaveText('Rainy day plans');
  await expect(drawer(page).locator('[aria-current="true"]')).toHaveText('Rainy day plans');
});

test('a reload asks the PC for the conversation again; one gone falls back to a new chat', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await recents(page).filter({ hasText: 'Trip packing list' }).click();
  await expect(page.getByTestId('turn')).toHaveCount(2);
  // A reload: the PC's copy of it, asked for once the page is up.
  await open(page);
  await ready(page, pc);
  await expect.poll(() => pc.of('open_chat').length).toBe(2);
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(page.getByTestId('title-sub')).toHaveText('Trip packing list');
  // Deleted on the PC meanwhile: said, and the next question starts a new one.
  delete pc.threads[41];
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('error')).toHaveText(
    "That chat isn't in Chat on your PC any more. Your next question starts a new one.",
  );
  await expect(page.getByTestId('turn')).toHaveCount(0);
  await type(page, pc, 'Start again?');
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Start again?', chat: 'new' });
});

test('a PC that never said where a question went: asked as before, with no chat', async ({ page }) => {
  const pc = await mockPc(page);
  await open(page);
  await ready(page, pc);
  await type(page, pc, 'What is the weather like?');
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'What is the weather like?' });
  expect((await snapshot(page)).chat).toBe(null);
});

test('a PC from before the drawer: no ☰, questions as before', async ({ page }) => {
  const pc = await mockPc(page, { can: CAN_BEFORE_CHATS });
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('menu')).toBeHidden();
  await expect(page.getByTestId('plus')).toBeHidden();
  // Even with a conversation remembered from a newer PC, nothing about it is sent.
  await page.evaluate(() => localStorage.setItem('yap.link.chat', '41'));
  await open(page);
  await ready(page, pc);
  await type(page, pc, 'Hello?');
  expect(pc.of('ask').at(-1)).toEqual({ type: 'ask', text: 'Hello?' });
  expect(pc.of('open_chat')).toHaveLength(0);
});
