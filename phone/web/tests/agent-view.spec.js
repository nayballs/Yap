// An agent's live view on the phone (Remote Control, wave 2; agent.js): a
// pill opens it over its chat — its status, cost and to-dos, its steps and
// what it said, its files with +N −M and one diff at a time (Wrap), its
// commands — fed by the PC's snapshot and then its numbered live events
// (a number missing, or `agent_resync`: the snapshot again), older steps on
// request; Stop, Continue, Undo after the phone's own confirm, "Tell the
// agent…" in the composer; ✕ back to the chat, the agent carrying on.
// Yap on the PC mocked with the wire's own examples (support/code-pc.js).
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, open, ready, snapshot } from './support/harness.js';
import { mockCodePc, brief, out, record, logLines, AGENT_ID } from './support/code-pc.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

const RUNNING = brief({ status: 'running', now: 'Adding the schedule', endedTs: 0, reportMessageId: undefined, turnList: [{ n: 1, title: 'Add a watering schedule', status: 'running', messageId: 'chat:12:0' }] });
const view = (page) => page.getByTestId('agent-view');

/** Chat 12 opened, then its agent's pill. */
async function openAgent(page) {
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await page.locator('[data-testid="code-chat"][data-id="12"]').click();
  await page.getByTestId('agent-pill').click();
  await expect(view(page)).toBeVisible();
}

/** A live event of the example agent: numbered `seq`, log line `line`. */
const live = (seq, line, event) => out('agent_event', { id: AGENT_ID, seq, line, event: { id: AGENT_ID, status: 'running', folder: 'garden-planner', conversation: 12, turn: 1, ts: 1730000200 + seq, ...event } });

test("a pill opens the agent's live view: status, cost, to-dos, steps and narration, files, commands; ✕ goes back and never stops it", async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [RUNNING] });
  await open(page);
  await ready(page, pc);
  await openAgent(page);
  await expect.poll(() => pc.of('agent_open')).toEqual([{ type: 'agent_open', id: AGENT_ID }]);
  await expect(page.getByTestId('thread')).toBeHidden();
  await expect(page.getByTestId('agent-title')).toHaveText('garden-planner');
  await expect(page.getByTestId('agent-status')).toHaveText('Running');
  await expect(page.getByTestId('agent-meta')).toContainText('Auto · Claude Sonnet 5.5');
  await expect(page.getByTestId('agent-cost')).toHaveText('Your Claude plan · ≈ $0.04 on the API');
  // The line it's on: the to-do in progress.
  await expect(page.getByTestId('agent-now')).toHaveText('Running npm test');
  const todos = page.getByTestId('agent-todos');
  await expect(todos).toContainText('1 of 3 done');
  await expect(todos.locator('.todo')).toHaveCount(3);
  await expect(todos.locator('.todo.in_progress')).toHaveText(/Adding the schedule/);
  // Its steps, and what it said between them.
  await expect(page.getByTestId('agent-step')).toHaveText(['Reading notes.md', 'Editing notes.md', 'Running npm testRun the tests']);
  await expect(page.getByTestId('agent-said')).toHaveText('I will add a table with a row for each bed and the days to water it.');
  // Its files, with what changed in each (the PC's counts; the lines on request).
  const file = page.getByTestId('agent-file');
  await expect(file).toHaveCount(1);
  await expect(file).toContainText('notes.md');
  await expect(file).toContainText('+4');
  await expect(file).toContainText('−1');
  await expect(page.getByTestId('agent-command')).toHaveText(['Run the testsnpm test']);
  // The composer is "Tell the agent…"; no pictures, model or hands-free talk there.
  await expect(page.getByTestId('input')).toHaveAttribute('placeholder', 'Tell the agent…');
  await expect(page.getByTestId('model-chip')).toBeHidden();
  await expect(page.getByTestId('call')).toBeHidden();
  await expect(page.getByTestId('coderow')).toBeHidden();
  await expect(page.getByTestId('agent-stop')).toBeVisible();
  // ✕: back to the chat; the agent carries on.
  await page.getByTestId('agent-close').click();
  await expect(view(page)).toBeHidden();
  await expect(page.getByTestId('thread')).toBeVisible();
  await expect.poll(() => pc.of('agent_close').length).toBe(1);
  expect(pc.of('agent_stop')).toEqual([]);
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('live events follow, in order; a number missing or the PC saying so opens it again, merged by its log lines', async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [RUNNING] });
  await open(page);
  await ready(page, pc);
  await openAgent(page);
  await expect(page.getByTestId('agent-step')).toHaveCount(3);
  pc.send(live(1, 7, { step: 'Writing the schedule into notes.md' }));
  pc.send(live(2, 8, { said: 'The tests pass.' }));
  await expect(page.getByTestId('agent-step')).toHaveCount(4);
  await expect(page.getByTestId('agent-step').last()).toHaveText('Writing the schedule into notes.md');
  await expect(page.getByTestId('agent-said').last()).toHaveText('The tests pass.');
  // Number 4 before 3: something went missing, so the snapshot again.
  const opens = pc.of('agent_open').length;
  pc.send(live(4, 10, { step: 'Reading package.json' }));
  await expect.poll(() => pc.of('agent_open').length).toBe(opens + 1);
  // The PC's own word that the phone fell behind: again.
  pc.send(out('agent_resync', { id: AGENT_ID }));
  await expect.poll(() => pc.of('agent_open').length).toBe(opens + 2);
  // What came live stays (merged with the snapshot by line).
  await expect(page.getByTestId('agent-step')).toHaveText(['Reading notes.md', 'Editing notes.md', 'Running npm testRun the tests', 'Writing the schedule into notes.md']);
  // Done: its status turns, and the view reads it again.
  const done = { status: 'done', endedTs: 1730000400, text: 'I added a watering schedule.' };
  pc.codeSide.record = record({ ...done, turnList: [{ ...record().turnList[0], ...done }] });
  pc.send(out('agent_event', { id: AGENT_ID, seq: 1, line: 11, event: { id: AGENT_ID, status: 'done', folder: 'garden-planner', turn: 1, cost: 0.06, ts: 1730000400 } }));
  await expect(page.getByTestId('agent-status')).toHaveText('Done');
  await expect(page.getByTestId('agent-cost')).toHaveText('Your Claude plan · ≈ $0.06 on the API');
  await expect(page.getByTestId('agent-stop')).toHaveCount(0);
  await expect(page.getByTestId('agent-text')).toContainText('I added a watering schedule.');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('older steps on request, and one file’s diff at a time, with Wrap', async ({ page }) => {
  // The snapshot: its last events (lines 101–106), older ones left on the PC.
  const lines = logLines().map((l) => ({ ...l, line: l.line + 100 }));
  const older = [
    { line: 99, event: { id: AGENT_ID, status: 'running', folder: 'garden-planner', turn: 1, step: 'Looking at the folder', ts: 1730000100 } },
    { line: 100, event: { id: AGENT_ID, status: 'running', folder: 'garden-planner', turn: 1, step: 'Reading README.md', ts: 1730000101 } },
  ];
  const pc = await mockCodePc(page, { agents: [RUNNING], log: lines, more: true, older });
  await open(page);
  await ready(page, pc);
  await openAgent(page);
  await expect(page.getByTestId('agent-step')).toHaveCount(3);
  await page.getByTestId('agent-older').click();
  await expect.poll(() => pc.of('agent_log')).toEqual([{ type: 'agent_log', id: AGENT_ID, before: 101 }]);
  await expect(page.getByTestId('agent-step').first()).toHaveText('Looking at the folder');
  await expect(page.getByTestId('agent-step')).toHaveCount(5);
  await expect(page.getByTestId('agent-older')).toHaveCount(0);
  // A file's diff: asked for, its lines marked.
  await page.getByTestId('agent-file').click();
  await expect.poll(() => pc.of('agent_diff')).toEqual([{ type: 'agent_diff', id: AGENT_ID, file: 'notes.md', turn: 1 }]);
  const diff = page.getByTestId('agent-diff');
  await expect(diff).toBeVisible();
  const marks = await diff.getByTestId('diff-line').evaluateAll((ls) => ls.map((l) => l.dataset.mark));
  expect(marks).toEqual(['gap', 'ctx', 'del', 'add', 'add', 'add', 'add']);
  await expect(diff.getByTestId('diff-line').nth(2)).toContainText('Water when dry');
  // Wrap folds long lines instead of scrolling them sideways.
  await expect(diff).toHaveAttribute('data-wrap', 'false');
  await diff.getByTestId('agent-diff-wrap').click();
  await expect(page.getByTestId('agent-diff')).toHaveAttribute('data-wrap', 'true');
  // Tapped again: closed.
  await page.getByTestId('agent-file').click();
  await expect(page.getByTestId('agent-diff')).toHaveCount(0);
  // The page never scrolls sideways.
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('Stop while it works; Continue once stopped', async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [RUNNING] });
  await open(page);
  await ready(page, pc);
  await openAgent(page);
  await page.getByTestId('agent-stop').click();
  await expect.poll(() => pc.of('agent_stop')).toEqual([{ type: 'agent_stop', id: AGENT_ID }]);
  await expect(page.getByTestId('agent-note')).toHaveText('Stopping the agent in garden-planner.');
  pc.codeSide.record = record({ status: 'stopped', endedTs: 1730000300 });
  pc.send(live(1, 7, { status: 'stopped', step: 'Stopped' }));
  await expect(page.getByTestId('agent-status')).toHaveText('Stopped');
  await page.getByTestId('agent-continue').click();
  await expect.poll(() => pc.of('agent_continue')).toEqual([{ type: 'agent_continue', id: AGENT_ID }]);
  await expect(page.getByTestId('agent-note')).toHaveText('Continuing the agent in garden-planner.');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test("Undo: the phone's own confirm first, with the files it puts back — Cancel sends nothing", async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [brief()], record: record({ status: 'done', endedTs: 1730000300, text: 'Done.' }) });
  await open(page);
  await ready(page, pc);
  await openAgent(page);
  await expect(page.getByTestId('agent-status')).toHaveText('Done');
  await page.getByTestId('agent-undo').click();
  const sheet = page.getByTestId('undo-sheet');
  await expect(sheet).toBeVisible();
  await expect(sheet.locator('#undo-title')).toHaveText('Put back 1 file in garden-planner?');
  await expect(sheet.getByTestId('undo-files')).toHaveText('notes.md');
  await sheet.getByTestId('undo-cancel').click();
  await expect(sheet).toBeHidden();
  expect(pc.of('agent_undo')).toEqual([]);
  await page.getByTestId('agent-undo').click();
  await sheet.getByTestId('undo-go').click();
  await expect.poll(() => pc.of('agent_undo')).toEqual([{ type: 'agent_undo', id: AGENT_ID, sure: true }]);
  await expect(page.getByTestId('agent-note')).toHaveText('Put back 1 file in garden-planner.');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('Tell the agent…: the composer’s words go to the agent, never as a question to Chat', async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [RUNNING] });
  await open(page);
  await ready(page, pc);
  await openAgent(page);
  await page.getByTestId('input').fill('Add the herbs bed too');
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('agent_message')).toEqual([{ type: 'agent_message', id: AGENT_ID, text: 'Add the herbs bed too' }]);
  expect(pc.of('ask')).toEqual([]);
  await expect(page.getByTestId('input')).toHaveValue('');
  await expect(page.getByTestId('agent-note')).toHaveText('Passed that to the agent in garden-planner.');
  // Refused by the PC: why, in the view, and the words back in the box.
  pc.codeSide.auto.act = false;
  await page.getByTestId('input').fill('And the roses');
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('agent_message').length).toBe(2);
  pc.send({ type: 'error', message: 'Slow down a little: Yap on your PC is still doing the last one.' });
  await expect(page.getByTestId('agent-error')).toHaveText('Slow down a little: Yap on your PC is still doing the last one.');
  await expect(page.getByTestId('input')).toHaveValue('And the roses');
  await expect(page.getByTestId('error')).toBeHidden();
  expect((await snapshot(page)).code.agent).toMatchObject({ id: AGENT_ID, busy: '' });
  expect(pc.codeSide.wireErrors).toEqual([]);
});
