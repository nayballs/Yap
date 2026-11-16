// Retry and its versions on the phone (16 Nov 2026): a chat's messages
// with versions show the PC's ‹ k/n › (`thread`'s `version`), and ‹ › here
// switches the PC's version shown (`show_version`: one version shown a
// conversation, the last switch wins, wherever it was made); `versions`
// from the PC (another screen switched, retried or edited) asks for the
// thread again, once nothing of this phone's is under way there; ↻ Retry
// under an answer the PC says may be asked again (`retry`) asks its question
// again, the old answer kept as a version, and the PC saying no puts the
// thread back as it was; a PC from before shows neither. Yap on the PC
// mocked.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, type, snapshot, sentences, CAN } from './support/harness.js';

/** A PC with Retry and versions on the phone (`ready.can`). */
const CAN_VERSIONS = [...CAN, 'versions', 'retry'];

/**
 * A chat on the PC whose first question was edited once (‹ 2/2 › on it)
 * and its answer retried twice (‹ 3/3 ›), each answer one ↻ may ask again.
 * Fictional, as everything in this public repo.
 */
const shown = () => ({
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: 'What should I pack for three days of hiking in the rain?', version: { at: 0, shown: 2, count: 2 } },
    { role: 'assistant', text: 'Layers: a waterproof jacket, two base layers and spare socks.', id: 'chat:41:1', version: { at: 1, shown: 3, count: 3 }, retry: true },
    { role: 'user', text: 'Is a 35-litre rucksack enough?' },
    { role: 'assistant', text: 'Yes, for three days if you stay in huts.', id: 'chat:41:3', retry: true },
  ],
});
/** …with its answer's second version shown instead (what followed the third went with it). */
const second = () => ({
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: 'What should I pack for three days of hiking in the rain?', version: { at: 0, shown: 2, count: 2 } },
    { role: 'assistant', text: 'A rain jacket, overtrousers and a dry bag.', id: 'chat:41:1', version: { at: 1, shown: 2, count: 3 }, retry: true },
  ],
});

/** The phone in chat 41 from the start (`yap.link.chat`): it asks for it once it's up. */
async function inChat(page, opts = {}) {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem('in-chat')) {
      localStorage.setItem('yap.link.chat', '41');
      sessionStorage.setItem('in-chat', '1');
    }
  });
  const pc = await mockPc(page, { can: CAN_VERSIONS, threads: { 41: shown() }, ...opts });
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('turn')).toHaveCount(2);
  return pc;
}

const turn = (page, n) => page.getByTestId('turn').nth(n);

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

test("a message's versions show the PC's ‹ k/n ›, and ‹ › switches the version shown there", async ({ page }) => {
  const pc = await inChat(page, {
    onText: (m, conn, pc) => {
      // The PC switches it and answers with the chat as it is now.
      if (m.type === 'show_version') {
        pc.threads[41] = m.place === 2 ? second() : shown();
        pc.sendTo(conn, { type: 'thread', id: 41, ...pc.threads[41], more: false });
      }
    },
  });
  // The edited question: ‹ 2/2 › under its bubble; the retried answer: ‹ 3/3 › first in its foot.
  const q = turn(page, 0).getByTestId('question-versions');
  const a = turn(page, 0).getByTestId('answer-versions');
  await expect(q.getByTestId('version-count')).toHaveText('2/2');
  await expect(q).toHaveAttribute('aria-label', 'Version 2 of 2');
  await expect(a.getByTestId('version-count')).toHaveText('3/3');
  await expect(a).toHaveAttribute('aria-label', 'Version 3 of 3');
  // ‹ at the first, › at the last asleep.
  await expect(q.getByTestId('version-next')).toBeDisabled();
  await expect(q.getByTestId('version-prev')).toBeEnabled();
  await expect(a.getByTestId('version-next')).toBeDisabled();
  // A message with none has none; ↻ under each answer the PC says may be asked again.
  await expect(turn(page, 1).getByTestId('question-versions')).toHaveCount(0);
  await expect(turn(page, 1).getByTestId('answer-versions')).toHaveCount(0);
  await expect(page.getByTestId('retry')).toHaveCount(2);
  // The foot's order, as the PC's: the versions, Copy, ▶, ↻.
  await expect(turn(page, 0).locator('.msgfoot > *')).toHaveCount(4);
  await expect(turn(page, 0).locator('.msgfoot > :first-child')).toHaveAttribute('data-testid', 'answer-versions');
  await expect(turn(page, 0).locator('.msgfoot > :last-child')).toHaveAttribute('data-testid', 'retry');

  // ‹ on the answer: the PC shows version 2 (here and on the PC), what followed version 3 goes with it.
  await a.getByTestId('version-prev').click();
  await expect.poll(() => pc.of('show_version')).toEqual([{ type: 'show_version', chat: 41, at: 1, place: 2 }]);
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(page.getByTestId('answer')).toHaveText('A rain jacket, overtrousers and a dry bag.');
  await expect(turn(page, 0).getByTestId('answer-versions').getByTestId('version-count')).toHaveText('2/3');
  await expect(turn(page, 0).getByTestId('answer-versions').getByTestId('version-next')).toBeEnabled();
  // › back to version 3: the thread as it was.
  await turn(page, 0).getByTestId('answer-versions').getByTestId('version-next').click();
  await expect.poll(() => pc.of('show_version').at(-1)).toEqual({ type: 'show_version', chat: 41, at: 1, place: 3 });
  await expect(page.getByTestId('turn')).toHaveCount(2);
  // The page keeps them as the PC sent them (a reload asks the PC again anyway).
  const snap = await snapshot(page);
  expect(snap.turns[0]).toMatchObject({ qv: { at: 0, shown: 2, count: 2 }, av: { at: 1, shown: 3, count: 3 }, retry: true });
});

test('‹ › refused while an answer is being written there: the version shown stays, and why', async ({ page }) => {
  const why = 'Wait for the answer being written, or stop it first.';
  const pc = await inChat(page, {
    onText: (m, conn, pc) => {
      if (m.type === 'show_version') pc.sendTo(conn, { type: 'error', message: why });
    },
  });
  await turn(page, 0).getByTestId('answer-versions').getByTestId('version-prev').click();
  await expect(page.getByTestId('error')).toHaveText(why);
  await expect(turn(page, 0).getByTestId('answer-versions').getByTestId('version-count')).toHaveText('3/3');
  await expect(page.getByTestId('turn')).toHaveCount(2);
  // Awake again at once: another tap goes.
  await turn(page, 0).getByTestId('answer-versions').getByTestId('version-prev').click();
  await expect.poll(() => pc.of('show_version').length).toBe(2);
});

test('a version switched on another screen: the thread again, at once or once the question here is answered', async ({ page }) => {
  const pc = await inChat(page);
  const opens = () => pc.of('open_chat').length;
  const before = opens();
  // Idle: asked for at once.
  pc.threads[41] = second();
  pc.send({ type: 'versions', chat: 41 });
  await expect.poll(opens).toBe(before + 1);
  await expect(page.getByTestId('answer')).toHaveText('A rain jacket, overtrousers and a dry bag.');
  // Another chat's: nothing.
  pc.send({ type: 'versions', chat: 38 });
  await page.waitForTimeout(300);
  expect(opens()).toBe(before + 1);
  // A question of this phone's under way here: not drawn over it — asked for once it's answered.
  await type(page, pc, 'And for five days?');
  pc.send({ type: 'heard', text: 'And for five days?' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.threads[41] = shown();
  pc.send({ type: 'versions', chat: 41 });
  await expect.poll(async () => (await snapshot(page)).threadAgain).toBe(true);
  await page.waitForTimeout(300);
  expect(opens()).toBe(before + 1);
  pc.send({ type: 'answer', text: 'Add a second base layer.', final: true, id: 'chat:41:3' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect.poll(opens).toBe(before + 2);
  await expect(page.getByTestId('turn')).toHaveCount(2);
  expect((await snapshot(page)).threadAgain).toBe(false);
});

test('↻ Retry: its question asked again, what followed leaves the thread, then the new answer with its ‹ k/n ›', async ({ page }) => {
  const pc = await inChat(page);
  // ↻ on the first answer: the second question and its answer went with the old version.
  await turn(page, 0).getByTestId('retry').click();
  await expect.poll(() => pc.of('retry')).toEqual([{ type: 'retry', id: 'chat:41:1' }]);
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(page.getByTestId('heard')).toHaveText('What should I pack for three days of hiking in the rain?');
  await expect(page.getByTestId('answer')).toHaveCount(0);
  // Working it out (Chat's activity line, as for a typed question); ‹ › and ↻ asleep meanwhile.
  await expect(page.getByTestId('step')).toBeVisible();
  expect((await snapshot(page)).now).toBe('working');
  await expect(turn(page, 0).getByTestId('question-versions').getByTestId('version-prev')).toBeDisabled();
  // The PC answers it again, on screen (typed: nothing said)…
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'Pack a', final: false });
  await expect(page.getByTestId('answer')).toHaveText('Pack a');
  pc.send({ type: 'answer', text: 'Pack a rain jacket, overtrousers and gaiters.', final: true, id: 'chat:41:1', retry: true });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('answer')).toHaveText('Pack a rain jacket, overtrousers and gaiters.');
  expect(pc.of('read_aloud')).toEqual([]);
  // …and says its versions changed: the thread again, the new answer's ‹ 4/4 › on it.
  const opens = pc.of('open_chat').length;
  pc.threads[41] = {
    title: 'Trip packing list',
    messages: [
      { role: 'user', text: 'What should I pack for three days of hiking in the rain?', version: { at: 0, shown: 2, count: 2 } },
      { role: 'assistant', text: 'Pack a rain jacket, overtrousers and gaiters.', id: 'chat:41:1', version: { at: 1, shown: 4, count: 4 }, retry: true },
    ],
  };
  pc.send({ type: 'versions', chat: 41 });
  await expect.poll(() => pc.of('open_chat').length).toBe(opens + 1);
  await expect(turn(page, 0).getByTestId('answer-versions').getByTestId('version-count')).toHaveText('4/4');
  await expect(turn(page, 0).getByTestId('retry')).toBeEnabled();
});

test('↻ refused by the PC (a Code chat, an answer being written): the answer and what followed it back, and why', async ({ page }) => {
  const why = "In a Code chat, Retry is on your PC: there it rewinds Claude's session first.";
  const pc = await inChat(page, {
    onText: (m, conn, pc) => {
      if (m.type === 'retry') {
        pc.sendTo(conn, { type: 'error', message: why });
        pc.sendTo(conn, { type: 'state', phase: 'idle' });
      }
    },
  });
  await turn(page, 0).getByTestId('retry').click();
  await expect(page.getByTestId('error')).toHaveText(why);
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(page.getByTestId('answer').first()).toHaveText('Layers: a waterproof jacket, two base layers and spare socks.');
  await expect(turn(page, 0).getByTestId('answer-versions').getByTestId('version-count')).toHaveText('3/3');
  await expect(page.getByTestId('stop')).toBeHidden();
  expect((await snapshot(page)).now).toBe('idle');
  // ↻ awake again: another tap goes.
  await expect(turn(page, 0).getByTestId('retry')).toBeEnabled();
});

test('‹ › or ↻ while ▶ is said: the voice stops first', async ({ page }) => {
  const pc = await inChat(page);
  await turn(page, 1).getByTestId('read-aloud').click();
  await expect.poll(() => pc.of('read_aloud').length).toBe(1);
  pc.send({ type: 'state', phase: 'idle' });
  pc.send({ type: 'state', phase: 'speaking' });
  pc.send({ type: 'speak_start', rate: 24000 });
  for (const f of sentences(3000)) pc.sendBinary(f);
  await expect.poll(async () => (await snapshot(page)).now).toBe('speaking');
  await turn(page, 0).getByTestId('answer-versions').getByTestId('version-prev').click();
  await expect.poll(() => pc.of('show_version').length).toBe(1);
  const types = pc.types();
  expect(types.lastIndexOf('interrupt')).toBeGreaterThan(types.lastIndexOf('read_aloud'));
  expect(types.lastIndexOf('interrupt')).toBeLessThan(types.lastIndexOf('show_version'));
});

test('a PC from before Retry and versions: neither shown, whatever a thread carries', async ({ page }) => {
  await inChat(page, { can: CAN });
  await expect(page.getByTestId('question-versions')).toHaveCount(0);
  await expect(page.getByTestId('answer-versions')).toHaveCount(0);
  await expect(page.getByTestId('retry')).toHaveCount(0);
  // Copy and ▶ as before.
  await expect(page.getByTestId('copy')).toHaveCount(2);
  await expect(page.getByTestId('read-aloud')).toHaveCount(2);
});
