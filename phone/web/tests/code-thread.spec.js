// A Code chat's thread on the phone (Remote Control, wave 2): the person's
// words as bubbles, Yap's answers with their agents' pills under them, an
// agent's report as a quiet card (never the person's bubble), a request
// sent there typed or by Talk, the report arriving live and said on the
// phone, a plan's Go ahead from its report, the Mode chip up to Auto
// (never Bypass; above the workspace's cap greyed), the Environment sheet.
// Yap on the PC mocked with the wire's own examples (support/code-pc.js).
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, open, ready, sentences } from './support/harness.js';
import { mockCodePc, brief, out, AGENT_ID, CODE } from './support/code-pc.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

/** Chat 12 opened from the drawer's Code side. */
async function openChat12(page) {
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await page.locator('[data-testid="code-chat"][data-id="12"]').click();
  await expect(page.getByTestId('turn')).toHaveCount(2);
}

test("a Code chat's thread: bubbles for the person's words, an agent pill under the answer, the report as a quiet card", async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  await expect.poll(() => pc.of('open_chat').at(-1)).toEqual({ type: 'open_chat', id: 12 });
  // Only the person's own words are a bubble.
  await expect(page.getByTestId('heard')).toHaveText(['Add a watering schedule to notes.md']);
  // Under the answer that started it, its agent's pill.
  const pill = page.getByTestId('agent-pill');
  await expect(pill).toHaveCount(1);
  await expect(pill).toContainText('garden-planner');
  await expect(pill).toContainText('Add a watering schedule');
  await expect(pill).toContainText('Done');
  await expect(pill).toHaveAttribute('data-id', AGENT_ID);
  await expect(page.getByTestId('turn').first().getByTestId('agent-pill')).toHaveCount(1);
  // The report: a card with what it changed, and Open.
  const card = page.getByTestId('report-card');
  await expect(card).toHaveCount(1);
  await expect(card.getByTestId('report-title')).toHaveText('Agent done · garden-planner');
  await expect(card).toContainText('Changed notes.md');
  await expect(card.getByTestId('report-go-ahead')).toHaveCount(0);
  await expect(page.getByTestId('turn').last().getByTestId('answer')).toHaveText('Done: I added a watering schedule to notes.md, with a row for each bed.');
  // The composer: the Code teal, its workspace and Mode.
  await expect(page.getByTestId('composer')).toHaveAttribute('data-code', 'true');
  await expect(page.getByTestId('workspace-chip')).toHaveText('garden-planner');
  await expect(page.getByTestId('mode-chip')).toHaveText('Auto');
  await expect(page.getByTestId('input')).toHaveAttribute('placeholder', 'Ask for a change, or hold to talk');
  // Open: the agent's live view, at that report's turn.
  await card.getByTestId('report-open').click();
  await expect(page.getByTestId('agent-view')).toBeVisible();
  await expect.poll(() => pc.of('agent_open').at(-1)).toEqual({ type: 'agent_open', id: AGENT_ID });
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('a request sent into the Code chat, typed and by Talk; its report comes live, as a card, and is said on the phone', async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  // Typed: asked in this chat.
  await page.getByTestId('input').fill('Now add the herbs bed too');
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('ask').at(-1)).toMatchObject({ type: 'ask', text: 'Now add the herbs bed too', chat: 12 });
  pc.send({ type: 'answer', text: "On it: I've passed that to the agent in garden-planner.", final: true, id: 'chat:12:5' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('turn')).toHaveCount(3);
  // Talk: its words in the box (this chat's), then sent as said.
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').at(-1)).toMatchObject({ mode: 'words', chat: 12 });
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(1);
  pc.send({ type: 'state', phase: 'transcribing' });
  pc.send({ type: 'words', text: 'And water the tomatoes twice a week' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('input')).toHaveValue('And water the tomatoes twice a week');
  await page.getByTestId('send').click();
  await expect.poll(() => pc.of('ask').at(-1)).toMatchObject({ text: 'And water the tomatoes twice a week', chat: 12, speak: true });
  pc.send({ type: 'answer', text: 'Passed that on too.', final: true, id: 'chat:12:7' });
  pc.send({ type: 'state', phase: 'idle' });
  await expect(page.getByTestId('turn')).toHaveCount(4);
  // The agent's turn reported: its marker and answer come as messages, and it's said here.
  pc.send(out('report', { conversation: 12, agent: AGENT_ID, turn: 2, status: 'done' }));
  pc.send(out('chat_message', { conversation: 12, message: { role: 'user', text: "The agent in garden-planner: it's done.\nFiles it changed: notes.md (backed up first).", kind: 'agentReport' } }));
  pc.send({ type: 'chat_message', conversation: 12, message: { role: 'assistant', text: 'Done: the herbs bed and the tomatoes are in the schedule.', id: 'chat:12:9' } });
  await expect(page.getByTestId('report-card')).toHaveCount(2);
  await expect(page.getByTestId('turn').last().getByTestId('answer')).toHaveText('Done: the herbs bed and the tomatoes are in the schedule.');
  await expect(page.getByTestId('heard')).toHaveCount(3);
  // (Wave 1 says the report on the phone that asked: Yap's voice down this socket.)
  pc.send({ type: 'speak_start', rate: 24000 });
  for (const f of sentences(900)) pc.sendBinary(f);
  pc.send({ type: 'speak_end' });
  await expect(page.getByTestId('turn').last().locator('.speaking')).toBeVisible();
  await expect(page.getByTestId('turn').last().locator('.speaking')).toBeHidden({ timeout: 10_000 });
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test("a plan's report: Go ahead, once, from its card", async ({ page }) => {
  const plan = brief({ plan: true, planAllowed: false });
  const pc = await mockCodePc(page, {
    agents: [plan],
    thread: {
      messages: [
        { role: 'user', text: 'Plan a watering schedule first' },
        { role: 'assistant', text: "On it: I've sent an agent to garden-planner, in Plan mode.", id: 'chat:12:1' },
        { role: 'user', text: "The agent in garden-planner: it made a plan.\nWhat it said: 1. A table per bed.", kind: 'agentReport' },
        { role: 'assistant', text: 'The plan is ready: a table per bed. Go ahead?', id: 'chat:12:3' },
      ],
      agents: [plan],
    },
  });
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  const card = page.getByTestId('report-card');
  await expect(card.getByTestId('report-title')).toHaveText("The agent's plan is ready · garden-planner");
  const go = card.getByTestId('report-go-ahead');
  await expect(go).toHaveText('Go ahead');
  await go.click();
  await expect.poll(() => pc.of('agent_go_ahead')).toEqual([{ type: 'agent_go_ahead', id: AGENT_ID }]);
  await expect(go).toHaveText('Going ahead');
  await expect(go).toBeDisabled();
  await expect(page.getByTestId('note')).toHaveText('The agent in garden-planner is carrying out its plan.');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('the Mode chip: Auto, Manual, Accept edits and Plan — never Bypass — picked as set_mode', async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  await page.getByTestId('mode-chip').click();
  const sheet = page.getByTestId('mode-sheet');
  await expect(sheet).toBeVisible();
  const rows = sheet.getByTestId('mode-row');
  await expect(rows).toHaveCount(4);
  expect(await rows.evaluateAll((r) => r.map((e) => e.dataset.mode))).toEqual(['auto', 'manual', 'acceptEdits', 'plan']);
  await expect(sheet).not.toContainText('Bypass permissions');
  await expect(rows.first()).toHaveAttribute('aria-checked', 'true');
  await sheet.locator('[data-mode="plan"]').click();
  await expect.poll(() => pc.of('set_mode')).toEqual([{ type: 'set_mode', chat: 12, mode: 'plan' }]);
  await expect(sheet).toBeHidden();
  await expect(page.getByTestId('mode-chip')).toHaveText('Plan');
  // Changed on the PC: the chip follows.
  pc.send(out('mode', { chat: 12, mode: 'manual', label: 'Manual', cap: undefined }));
  await expect(page.getByTestId('mode-chip')).toHaveText('Manual');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test("the Mode chip under a workspace's cap greys what's above it; a Bypass chat on the PC is amber, and the phone can lower it", async ({ page }) => {
  const pc = await mockCodePc(page, { thread: { code: { ...CODE, mode: 'bypassPermissions', modeLabel: 'Bypass permissions', cap: undefined } } });
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  const chip = page.getByTestId('mode-chip');
  await expect(chip).toHaveText('Bypass permissions');
  await expect(chip).toHaveAttribute('data-bypass', 'true');
  await chip.click();
  const sheet = page.getByTestId('mode-sheet');
  await expect(sheet.getByTestId('mode-bypass')).toContainText('From your phone, agents run at most at Auto');
  await expect(sheet.getByTestId('mode-row')).toHaveCount(4);
  await sheet.locator('[data-mode="manual"]').click();
  await expect.poll(() => pc.of('set_mode').at(-1)).toEqual({ type: 'set_mode', chat: 12, mode: 'manual' });
  await expect(chip).toHaveText('Manual');
  await expect(chip).toHaveAttribute('data-bypass', 'false');

  // A workspace that never runs above Manual: Accept edits and Auto greyed, with why.
  pc.threads[14] = { title: 'Fix the failing tests', messages: [], code: { ...CODE, workspace: 'recipe-box', path: 'D:\\Projects\\recipe-box', mode: 'manual', modeLabel: 'Manual', cap: 'manual', phoneMax: 'manual' }, agents: [], cards: [] };
  await page.getByTestId('menu').click();
  await page.locator('[data-testid="code-chat"][data-id="14"]').click();
  await expect(chip).toHaveText('Manual');
  await chip.click();
  await expect(sheet.locator('[data-mode="auto"]')).toBeDisabled();
  await expect(sheet.locator('[data-mode="acceptEdits"]')).toBeDisabled();
  await expect(sheet.locator('[data-mode="auto"]')).toContainText('Never above Manual in recipe-box');
  await expect(sheet.locator('[data-mode="plan"]')).toBeEnabled();
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('the workspace chip opens the Environment: the workspace and its folder, where it runs, its Mode, Remote Control', async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await openChat12(page);
  await page.getByTestId('workspace-chip').click();
  const env = page.getByTestId('env-sheet');
  await expect(env).toBeVisible();
  await expect(env.getByTestId('env-workspace')).toHaveText('garden-planner');
  await expect(env.getByTestId('env-path')).toHaveText('D:\\Projects\\garden-planner');
  await expect(env.getByTestId('env-pc')).toHaveText('Office PC');
  await expect(env.getByTestId('env-mode')).toHaveText('Auto');
  await expect(env.getByTestId('env-max')).toHaveText('At most Auto');
  await expect(env.getByTestId('env-remote')).toHaveText('On');
  await env.getByRole('button', { name: 'Done' }).click();
  await expect(env).toBeHidden();
});
