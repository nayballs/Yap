// Versions on the phone (16 Nov 2026): a chat's messages with versions show
// the PC's ‹ k/n › (`thread`'s `version`: an Edit's, or an older Retry's on
// an answer), and ‹ › here switches the PC's version shown (`show_version`:
// one version shown a conversation, the last switch wins, wherever it was
// made); `versions` from the PC (another screen switched or edited) asks for
// the thread again, once nothing of this phone's is under way there. Under
// an answer, as the PC's row: Copy, the speaker and its time — no ↻ Retry
// (taken out the same day), whatever a thread carries; a PC from before
// shows no versions. ✎ Edit (the same
// day): the pencil under the person's own question (`edit` on it: its time
// in Chat) puts it in the box to change, "Editing your message" over it;
// Send makes a new version on the PC (`edit {chat, at, ts, text}`) with its
// own answer, ‹ › switching between them; ✕ leaves it as it was, the box as
// it was before; the PC saying no puts the thread back and the words in the
// box; a version switched elsewhere meanwhile leaves the words as a new
// message; Talk's words in an edit go as said; none from a PC from before,
// nor in a Code chat. Yap on the PC mocked.
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, mockPc, open, ready, type, ask, snapshot, sentences, CAN } from './support/harness.js';
import { mockCodePc, codeThread, CAN_CODE } from './support/code-pc.js';

/** A PC with versions on the phone (`ready.can`). */
const CAN_VERSIONS = [...CAN, 'versions'];
/** Answers' times in Chat (seconds): 12 minutes ago, and 3 hours ago. */
const NOW = Math.floor(Date.now() / 1000);
const TWELVE_MIN = NOW - 12 * 60 - 5;
const THREE_H = NOW - 3 * 3600 - 5;

/**
 * A chat on the PC whose first question was edited once (‹ 2/2 › on it)
 * and its answer retried twice by an older Yap (‹ 3/3 ›), each answer with
 * its time in Chat. Fictional, as everything in this public repo.
 */
const shown = () => ({
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: 'What should I pack for three days of hiking in the rain?', version: { at: 0, shown: 2, count: 2 } },
    { role: 'assistant', text: 'Layers: a waterproof jacket, two base layers and spare socks.', id: 'chat:41:1', version: { at: 1, shown: 3, count: 3 }, ts: THREE_H },
    { role: 'user', text: 'Is a 35-litre rucksack enough?' },
    { role: 'assistant', text: 'Yes, for three days if you stay in huts.', id: 'chat:41:3', ts: TWELVE_MIN },
  ],
});
/** …with its answer's second version shown instead (what followed the third went with it). */
const second = () => ({
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: 'What should I pack for three days of hiking in the rain?', version: { at: 0, shown: 2, count: 2 } },
    { role: 'assistant', text: 'A rain jacket, overtrousers and a dry bag.', id: 'chat:41:1', version: { at: 1, shown: 2, count: 3 }, ts: THREE_H },
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
  // A message with none has none.
  await expect(turn(page, 1).getByTestId('question-versions')).toHaveCount(0);
  await expect(turn(page, 1).getByTestId('answer-versions')).toHaveCount(0);
  // The foot's order, as the PC's: the versions, Copy, ▶, its time. No ↻.
  await expect(page.getByTestId('retry')).toHaveCount(0);
  await expect(turn(page, 0).locator('.msgfoot > *')).toHaveCount(4);
  await expect(turn(page, 0).locator('.msgfoot > :first-child')).toHaveAttribute('data-testid', 'answer-versions');
  await expect(turn(page, 0).locator('.msgfoot > :last-child')).toHaveAttribute('data-testid', 'answer-when');
  await expect(turn(page, 0).getByTestId('answer-when')).toHaveText('3 hours ago');
  await expect(turn(page, 1).getByTestId('answer-when')).toHaveText('12 minutes ago');

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
  expect(snap.turns[0]).toMatchObject({ qv: { at: 0, shown: 2, count: 2 }, av: { at: 1, shown: 3, count: 3 } });
  expect(snap.turns[0].retry).toBeUndefined();
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

test('no ↻ under an answer, whatever a thread carries; a new answer says "just now"', async ({ page }) => {
  // (A PC from earlier the same day said `retry` on its answers: never offered now.)
  const old = shown();
  for (const m of old.messages) if (m.role === 'assistant') m.retry = true;
  const pc = await inChat(page, { can: [...CAN_VERSIONS, 'retry'], threads: { 41: old } });
  await expect(page.getByTestId('retry')).toHaveCount(0);
  await expect(page.getByTestId('copy')).toHaveCount(2);
  await expect(page.getByTestId('read-aloud')).toHaveCount(2);
  // A question asked here: its answer's row, its time now.
  await type(page, pc, 'And for five days?');
  pc.send({ type: 'heard', text: 'And for five days?' });
  pc.send({ type: 'answer', text: 'Add a second base layer.', final: true, id: 'chat:41:5', retry: true });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(turn(page, 2).getByTestId('answer-when')).toHaveText('just now');
  await expect(turn(page, 2).getByTestId('retry')).toHaveCount(0);
});

test('‹ › while ▶ is said: the voice stops first', async ({ page }) => {
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

test('a PC from before versions: none shown, whatever a thread carries', async ({ page }) => {
  await inChat(page, { can: CAN });
  await expect(page.getByTestId('question-versions')).toHaveCount(0);
  await expect(page.getByTestId('answer-versions')).toHaveCount(0);
  await expect(page.getByTestId('retry')).toHaveCount(0);
  // Copy and ▶ as before.
  await expect(page.getByTestId('copy')).toHaveCount(2);
  await expect(page.getByTestId('read-aloud')).toHaveCount(2);
});

// ---------------------------------------------------------------- ✎ Edit

/** A PC with ✎ Edit on the phone too (`ready.can`). */
const CAN_EDIT = [...CAN_VERSIONS, 'edit'];
const Q1 = 'What should I pack for three days of hiking in the rain?';
const Q2 = 'Is a 35-litre rucksack enough?';
const A1 = 'Layers: a waterproof jacket, two base layers and spare socks.';
// Each question's time in Chat, which ✎ names it by (`edit` on it).
const Q1_TS = 1_731_772_800_100;
const Q2_TS = 1_731_772_800_300;
/** A chat whose two questions ✎ may edit. */
const editable = () => ({
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: Q1, edit: Q1_TS },
    { role: 'assistant', text: A1, id: 'chat:41:1' },
    { role: 'user', text: Q2, edit: Q2_TS },
    { role: 'assistant', text: 'Yes, for three days if you stay in huts.', id: 'chat:41:3' },
  ],
});
const EDITED = 'What should I pack for five days of hiking in the rain?';
const EDITED_TS = 1_731_772_800_900;
const EDITED_ANSWER = 'For five wet days: two jackets, gaiters and a dry bag for each day.';
/** …after the first question was edited from the phone: ‹ 2/2 › on it, the new answer its own. */
const edited = () => ({
  title: 'Trip packing list',
  messages: [
    { role: 'user', text: EDITED, edit: EDITED_TS, version: { at: 0, shown: 2, count: 2 } },
    { role: 'assistant', text: EDITED_ANSWER, id: 'chat:41:1' },
  ],
});
/** …and switched back (‹): the question as it was, with what followed it. */
const back = () => {
  const t = editable();
  t.messages[0].version = { at: 0, shown: 1, count: 2 };
  return t;
};
const strip = (page) => page.getByTestId('editing');

test('✎ puts your question in the box; Send makes a new version with its own answer, and ‹ › switches between them', async ({ page }) => {
  const pc = await inChat(page, {
    can: CAN_EDIT,
    threads: { 41: editable() },
    onText: (m, conn, pc) => {
      if (m.type === 'show_version') {
        pc.threads[41] = m.place === 1 ? back() : edited();
        pc.sendTo(conn, { type: 'thread', id: 41, ...pc.threads[41], more: false });
      }
    },
  });
  // ✎ under each of your questions, none under an answer; something half-written in the box.
  await expect(page.getByTestId('edit')).toHaveCount(2);
  await expect(turn(page, 0).locator('.qfoot').getByTestId('edit')).toHaveAttribute('aria-label', 'Edit');
  await expect(turn(page, 0).locator('.msgfoot').getByTestId('edit')).toHaveCount(0);
  await page.getByTestId('input').fill('Remember the map');
  await turn(page, 0).getByTestId('edit').click();
  // The question in the box, "Editing your message" over it, its bubble marked; + and hands-free talk aside.
  await expect(strip(page)).toBeVisible();
  await expect(strip(page)).toContainText('Editing your message');
  await expect(page.getByTestId('input')).toHaveValue(Q1);
  await expect(page.getByTestId('input')).toBeFocused();
  await expect(turn(page, 0).getByTestId('heard')).toHaveClass(/being-edited/);
  await expect(turn(page, 0).getByTestId('edit')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('plus')).toBeHidden();
  await expect(page.getByTestId('call')).toBeHidden();
  await expect(page.getByTestId('send')).toHaveAttribute('aria-label', 'Send the edited message');
  expect((await snapshot(page)).editing).toEqual({ at: 0, ts: Q1_TS });

  // Changed and sent: a new version of that question on the PC, never a new question.
  await page.getByTestId('input').fill(EDITED);
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('edit')).toEqual([{ type: 'edit', chat: 41, at: 0, ts: Q1_TS, text: EDITED }]);
  expect(pc.of('ask')).toEqual([]);
  // Here the question and what followed it leave (they went with the old version), the new words in their place,
  // Chat's activity line meanwhile; the box as it was before the edit.
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(page.getByTestId('heard')).toHaveText(EDITED);
  await expect(page.getByTestId('answer')).toHaveCount(0);
  await expect(page.getByTestId('step')).toBeVisible();
  await expect(strip(page)).toBeHidden();
  await expect(page.getByTestId('input')).toHaveValue('Remember the map');
  expect((await snapshot(page)).editing).toBe(null);
  // Its answer, on screen (typed: nothing said).
  pc.send({ type: 'heard', text: EDITED });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'For five wet days:', final: false });
  pc.send({ type: 'answer', text: EDITED_ANSWER, final: true, id: 'chat:41:1', edit: EDITED_TS });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('answer')).toHaveText(EDITED_ANSWER);
  expect(pc.of('read_aloud')).toEqual([]);
  // ✎ on the new words at once (the answer said their time in Chat).
  await expect(turn(page, 0).getByTestId('edit')).toBeEnabled();
  expect((await snapshot(page)).turns[0]).toMatchObject({ q: EDITED, edit: EDITED_TS });

  // Then the PC says the chat's versions changed: the thread again, ‹ 2/2 › on the question, then ✎ (the PC's order).
  const opens = pc.of('open_chat').length;
  pc.threads[41] = edited();
  pc.send({ type: 'versions', chat: 41 });
  await expect.poll(() => pc.of('open_chat').length).toBe(opens + 1);
  const qv = turn(page, 0).getByTestId('question-versions');
  await expect(qv.getByTestId('version-count')).toHaveText('2/2');
  await expect(turn(page, 0).locator('.qfoot > :first-child')).toHaveAttribute('data-testid', 'question-versions');
  await expect(turn(page, 0).locator('.qfoot > :last-child')).toHaveAttribute('data-testid', 'edit');
  // ‹: the question as it was, with its answer and what followed it (switched on the PC too).
  await qv.getByTestId('version-prev').click();
  await expect.poll(() => pc.of('show_version').at(-1)).toEqual({ type: 'show_version', chat: 41, at: 0, place: 1 });
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(turn(page, 0).getByTestId('heard')).toHaveText(Q1);
  await expect(turn(page, 0).getByTestId('answer')).toHaveText(A1);
  await expect(turn(page, 0).getByTestId('question-versions').getByTestId('version-count')).toHaveText('1/2');
  // ›: the edit's version again.
  await turn(page, 0).getByTestId('question-versions').getByTestId('version-next').click();
  await expect.poll(() => pc.of('show_version').at(-1)).toEqual({ type: 'show_version', chat: 41, at: 0, place: 2 });
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(page.getByTestId('heard')).toHaveText(EDITED);
});

test('✕, ✎ again or Escape: the question as it was, the box as it was before, nothing sent', async ({ page }) => {
  const pc = await inChat(page, { can: CAN_EDIT, threads: { 41: editable() } });
  await page.getByTestId('input').fill('Remember the map');
  await turn(page, 1).getByTestId('edit').click();
  await expect(page.getByTestId('input')).toHaveValue(Q2);
  await page.getByTestId('input').fill('Is a 45-litre rucksack enough?');
  await page.getByTestId('edit-cancel').click();
  await expect(strip(page)).toBeHidden();
  await expect(page.getByTestId('input')).toHaveValue('Remember the map');
  await expect(turn(page, 1).getByTestId('heard')).toHaveText(Q2);
  await expect(turn(page, 1).getByTestId('heard')).not.toHaveClass(/being-edited/);
  // ✎ again on the same question leaves it too.
  await turn(page, 1).getByTestId('edit').click();
  await expect(strip(page)).toBeVisible();
  await turn(page, 1).getByTestId('edit').click();
  await expect(strip(page)).toBeHidden();
  await expect(page.getByTestId('input')).toHaveValue('Remember the map');
  // Escape, on a keyboard.
  await turn(page, 0).getByTestId('edit').click();
  await expect(page.getByTestId('input')).toHaveValue(Q1);
  await page.getByTestId('input').press('Escape');
  await expect(strip(page)).toBeHidden();
  await expect(page.getByTestId('input')).toHaveValue('Remember the map');
  // Another question's ✎ while one's open: that one in the box instead, the draft still kept for after.
  await turn(page, 0).getByTestId('edit').click();
  await turn(page, 1).getByTestId('edit').click();
  await expect(page.getByTestId('input')).toHaveValue(Q2);
  expect((await snapshot(page)).editing).toEqual({ at: 2, ts: Q2_TS });
  await page.getByTestId('edit-cancel').click();
  await expect(page.getByTestId('input')).toHaveValue('Remember the map');
  expect(pc.of('edit')).toEqual([]);
  expect(pc.of('ask')).toEqual([]);
});

test('the PC says no: the question and what followed it back, the new words in the box to send again', async ({ page }) => {
  const why = 'Wait for the answer being written, or stop it first.';
  const pc = await inChat(page, {
    can: CAN_EDIT,
    threads: { 41: editable() },
    onText: (m, conn, pc) => {
      if (m.type === 'edit') {
        pc.sendTo(conn, { type: 'error', message: why });
        pc.sendTo(conn, { type: 'state', phase: 'idle' });
      }
    },
  });
  await turn(page, 0).getByTestId('edit').click();
  await page.getByTestId('input').fill(EDITED);
  const opens = pc.of('open_chat').length;
  await page.getByTestId('send').click();
  await expect(page.getByTestId('error')).toHaveText(why);
  await expect(page.getByTestId('turn')).toHaveCount(2);
  await expect(turn(page, 0).getByTestId('heard')).toHaveText(Q1);
  await expect(turn(page, 0).getByTestId('answer')).toHaveText(A1);
  // The edit open again with its words, after the thread came back as the PC has it.
  await expect.poll(() => pc.of('open_chat').length).toBe(opens + 1);
  await expect(strip(page)).toBeVisible();
  await expect(page.getByTestId('input')).toHaveValue(EDITED);
  expect((await snapshot(page)).editing).toEqual({ at: 0, ts: Q1_TS });
  await expect(page.getByTestId('stop')).toBeHidden();
  expect((await snapshot(page)).now).toBe('idle');
  // Send tries again.
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('edit').length).toBe(2);
});

test('a version switched on another screen while you edit: your words stay in the box, as a new message now', async ({ page }) => {
  const pc = await inChat(page, { can: CAN_EDIT, threads: { 41: editable() } });
  await turn(page, 0).getByTestId('edit').click();
  await page.getByTestId('input').fill(EDITED);
  // The PC (or another phone) shows another version of that question meanwhile.
  pc.threads[41] = {
    title: 'Trip packing list',
    messages: [
      { role: 'user', text: 'What do I need for a wet weekend?', edit: 1_731_772_800_050, version: { at: 0, shown: 1, count: 2 } },
      { role: 'assistant', text: 'A good coat and spare socks.', id: 'chat:41:1' },
    ],
  };
  pc.send({ type: 'versions', chat: 41 });
  await expect(page.getByTestId('turn')).toHaveCount(1);
  await expect(strip(page)).toBeHidden();
  await expect(page.getByTestId('input')).toHaveValue(EDITED);
  await expect(page.getByTestId('note')).toContainText('That message changed on your PC');
  // Sent now, it's a new question in the chat, never an edit of another.
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('ask').length).toBe(1);
  expect(pc.of('ask')[0]).toMatchObject({ type: 'ask', text: EDITED, chat: 41 });
  expect(pc.of('edit')).toEqual([]);
});

test("Talk's words added to an edit: sent as said", async ({ page }) => {
  const pc = await inChat(page, { can: CAN_EDIT, threads: { 41: editable() } });
  await turn(page, 1).getByTestId('edit').click();
  await page.getByTestId('input').fill('Is a 35-litre rucksack enough');
  // Talk while it's open: its words in the box at the caret, the edit still open.
  await ask(page, pc, 300);
  expect(pc.of('start').at(-1)).toMatchObject({ type: 'start', mode: 'words' });
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'words', text: 'for five days?' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('input')).toHaveValue('Is a 35-litre rucksack enough for five days?');
  await expect(strip(page)).toBeVisible();
  await page.getByTestId('send').click();
  await expect
    .poll(() => pc.of('edit'))
    .toEqual([{ type: 'edit', chat: 41, at: 2, ts: Q2_TS, text: 'Is a 35-litre rucksack enough for five days?', speak: true }]);
  // Asked as said: the bubble's 🎙.
  await expect(turn(page, 1).getByTestId('heard').locator('.viavoice')).toHaveCount(1);
});

test('✎ on a question just asked here, once its answer says it may be edited', async ({ page }) => {
  const pc = await inChat(page, { can: CAN_EDIT, threads: { 41: editable() } });
  await type(page, pc, 'And for a week?');
  // Not while it's being answered.
  await expect(turn(page, 2).getByTestId('edit')).toHaveCount(0);
  await expect(turn(page, 0).getByTestId('edit')).toBeDisabled();
  pc.send({ type: 'heard', text: 'And for a week?' });
  pc.send({ type: 'state', phase: 'thinking' });
  pc.send({ type: 'answer', text: 'Add a spare fleece.', final: true, id: 'chat:41:5', edit: 1_731_772_801_000 });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(turn(page, 2).getByTestId('edit')).toBeEnabled();
  await expect(turn(page, 0).getByTestId('edit')).toBeEnabled();
  await turn(page, 2).getByTestId('edit').click();
  await expect(page.getByTestId('input')).toHaveValue('And for a week?');
  await page.getByTestId('input').fill('And for ten days?');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('edit')).toEqual([{ type: 'edit', chat: 41, at: 4, ts: 1_731_772_801_000, text: 'And for ten days?' }]);
  await expect(page.getByTestId('turn')).toHaveCount(3);
  await expect(turn(page, 2).getByTestId('heard')).toHaveText('And for ten days?');
});

test('no ✎ from a PC from before Edit, whatever a thread carries', async ({ page }) => {
  await inChat(page, { can: CAN_VERSIONS, threads: { 41: editable() } });
  await expect(page.getByTestId('edit')).toHaveCount(0);
  // Copy as before.
  await expect(page.getByTestId('copy')).toHaveCount(2);
});

test('no ✎ in a Code chat: there Edit is a Rewind, on the PC', async ({ page }) => {
  await page.addInitScript(() => {
    if (!sessionStorage.getItem('in-chat')) {
      localStorage.setItem('yap.link.chat', '12');
      sessionStorage.setItem('in-chat', '1');
    }
  });
  // (Even were a thread to carry `edit`, as the PC never does in a Code chat.)
  const thread = codeThread();
  thread.messages[0].edit = 1_731_772_800_100;
  const pc = await mockCodePc(page, { can: [...CAN_CODE, 'versions', 'edit'], threads: { 12: thread } });
  await open(page);
  await ready(page, pc);
  await expect(page.getByTestId('heard').first()).toHaveText('Add a watering schedule to notes.md');
  await expect(page.getByTestId('coderow')).toBeVisible();
  await expect(page.getByTestId('edit')).toHaveCount(0);
});
