// Remote Control's Code side in the drawer (wave 2; README.md "The Code
// side"): with the PC saying Remote Control is on for this phone, ☰ has
// Chat | Code — Chat the plain conversations, Code the agents working and
// each workspace's Code chats with their status dots (core/chatStatus.js,
// the desktop's own rules), + New code chat; with it off, or a PC from
// before, the page is Chat as it was. Yap on the PC mocked with the wire's
// own examples (support/code-pc.js).
import { test, expect } from '@playwright/test';
import { servePage, seedDevice, open, ready, snapshot, RECENTS, CAN } from './support/harness.js';
import { mockCodePc, brief, out, AGENT_ID, CHATS, WORKSPACES } from './support/code-pc.js';

test.beforeEach(async ({ page }) => {
  await servePage(page);
  await seedDevice(page);
});

const drawer = (page) => page.getByTestId('drawer');

/** A brief of the example agent, at work in chat 12. */
const RUNNING = brief({ status: 'running', now: 'Adding a watering schedule', endedTs: 0, reportMessageId: undefined, turnList: [{ n: 1, title: 'Add a watering schedule', status: 'running', messageId: 'chat:12:0' }] });

test('Chat | Code: Chat lists the plain chats, Code the agents working and each workspace with its Code chats', async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [RUNNING] });
  await open(page);
  await ready(page, pc);
  // The PC is asked for the Code side as soon as it's on.
  await expect.poll(() => pc.of('code_list').length).toBeGreaterThan(0);
  await page.getByTestId('menu').click();
  await expect(drawer(page)).toBeVisible();
  await expect(page.getByTestId('sides')).toBeVisible();
  await expect(page.getByTestId('side-chat')).toHaveAttribute('aria-selected', 'true');
  // Chat: the plain conversations only (a Code chat is on the Code side).
  await expect(page.getByTestId('recent')).toHaveText(RECENTS.map((r) => r.title));
  // Code: the agent working first, then each workspace's chats.
  await page.getByTestId('side-code').click();
  await expect(page.getByTestId('side-code')).toHaveAttribute('aria-selected', 'true');
  await expect(page.getByTestId('code-side')).toBeVisible();
  const run = page.getByTestId('code-running-agent');
  await expect(run).toHaveCount(1);
  await expect(run).toContainText('garden-planner');
  await expect(run).toContainText('Adding a watering schedule');
  await expect(page.getByTestId('code-workspace')).toHaveCount(2);
  await expect(page.getByTestId('code-workspace').first()).toHaveAttribute('data-name', 'garden-planner');
  await expect(page.getByTestId('code-chat')).toHaveText(CHATS.map((c) => c.title));
  await expect(page.getByTestId('new-chat')).toHaveText('New code chat');
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('the dots, by the desktop’s own rules: working while an agent runs, needs you while a step waits, new when a report came', async ({ page }) => {
  const pc = await mockCodePc(page, { agents: [brief()] });
  await open(page);
  await ready(page, pc);
  await expect.poll(async () => (await snapshot(page)).code.on).toBe(true);
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await expect(page.getByTestId('code-chat')).toHaveCount(2);
  const dotOf = (id) => page.locator(`[data-testid="code-chat"][data-id="${id}"] [data-testid="code-chat-dot"]`);
  // Nothing yet: a first visit counts what's there as seen.
  await expect(dotOf(12)).toHaveAttribute('data-state', '');
  await expect(dotOf(14)).toHaveAttribute('data-state', '');
  // An agent at work in chat 12 (shown once it has worked 300 ms, as the desktop's).
  pc.status({ agents: [RUNNING] });
  await expect(dotOf(12)).toHaveAttribute('data-state', 'working');
  await expect(dotOf(12)).toHaveAttribute('aria-label', 'Working: an agent is running');
  // A step waiting in chat 14: needs you.
  pc.status({ agents: [RUNNING], approvals: [{ id: 7, conversation: 14, agent: 'ab12cd34' }] });
  await expect(dotOf(14)).toHaveAttribute('data-state', 'needs');
  await expect(dotOf(14)).toHaveAttribute('data-why', 'approve');
  // Chat 12's agent reports while the phone isn't in it: new.
  pc.status({ agents: [brief({ turnList: [{ n: 1, title: 'Add a watering schedule', status: 'done', messageId: 'chat:12:0', reportMessageId: 'chat:12:4' }], reportMessageId: 'chat:12:4' })] });
  await expect(dotOf(12)).toHaveAttribute('data-state', 'new');
  await expect(dotOf(12)).toHaveAttribute('data-why', 'report');
  // On the Chat side, the Code side's segment carries its strongest dot.
  await page.getByTestId('side-chat').click();
  await expect(page.getByTestId('side-code-dot')).toHaveAttribute('data-state', 'new');
  // Opening it clears it (seen, kept in this phone's own storage).
  await page.getByTestId('side-code').click();
  pc.threads[12].messages.push(
    { role: 'user', text: "The agent in garden-planner: it's done.\nFiles it changed: notes.md (backed up first).", kind: 'agentReport' },
    { role: 'assistant', text: 'All done.', id: 'chat:12:5' },
  );
  await page.locator('[data-testid="code-chat"][data-id="12"]').click();
  await expect(page.getByTestId('answer').last()).toHaveText('All done.');
  const seen = await page.evaluate(() => JSON.parse(localStorage.getItem('yap.link.seen')));
  expect(seen).toMatchObject({ v: 1, seen: { 12: 5 } });
  await page.getByTestId('menu').click();
  await expect(page.getByTestId('side-code')).toHaveAttribute('aria-selected', 'true');
  await expect(dotOf(12)).toHaveAttribute('data-state', '');
  // A plain chat answering (on the PC, say): its dot on the Chat side.
  pc.status({ answering: [RECENTS[1].id] });
  await page.getByTestId('side-chat').click();
  await expect(page.locator(`[data-testid="recent"][data-id="${RECENTS[1].id}"] [data-testid="recent-dot"]`)).toHaveAttribute('data-state', 'working');
});

test('+ New code chat: the workspaces most recently used first, then its first question makes the chat there', async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await page.getByTestId('new-chat').click();
  const sheet = page.getByTestId('workspace-sheet');
  await expect(sheet).toBeVisible();
  await expect(sheet.getByTestId('workspace')).toHaveCount(WORKSPACES.length);
  await expect(sheet.getByTestId('workspace').first()).toHaveAttribute('data-name', 'garden-planner');
  await sheet.getByTestId('workspace').filter({ hasText: 'recipe-box' }).click();
  await expect(sheet).toBeHidden();
  await expect(drawer(page)).toBeHidden();
  await expect.poll(() => pc.of('code_new')).toEqual([{ type: 'code_new', workspace: 'recipe-box' }]);
  // An empty Code chat: its workspace, what it's for, and prompts that fill the box (never sent).
  await expect(page.getByTestId('coderow')).toBeVisible();
  await expect(page.getByTestId('workspace-chip')).toHaveText('recipe-box');
  await expect(page.getByTestId('mode-chip')).toHaveText('Auto');
  await expect(page.locator('#empty-title')).toHaveText('Code · recipe-box');
  await expect(page.getByTestId('code-intro')).toContainText('Ask for a change, a fix or an explanation.');
  await page.getByTestId('code-prompt').first().click();
  await expect(page.getByTestId('input')).toHaveValue('Explain how this project is laid out');
  expect(pc.of('ask')).toHaveLength(0);
  // Sent: into a new chat, the PC's Code chat waiting for it.
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('ask').at(-1)).toMatchObject({ type: 'ask', text: 'Explain how this project is laid out', chat: 'new' });
  // Also from the workspace's own +.
  await page.getByTestId('menu').click();
  await page.getByTestId('code-workspace-new').first().click();
  await expect.poll(() => pc.of('code_new').at(-1)).toEqual({ type: 'code_new', workspace: 'garden-planner' });
  expect(pc.codeSide.wireErrors).toEqual([]);
});

test('a new Code chat still waiting for its first question: a reload tells the PC again, so it never becomes a plain chat', async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await page.getByTestId('code-workspace-new').nth(1).click();
  await expect.poll(() => pc.of('code_new')).toEqual([{ type: 'code_new', workspace: 'recipe-box' }]);
  await page.reload();
  await page.waitForFunction(() => !!window.__yapLink);
  await ready(page, pc);
  await expect.poll(() => pc.of('code_new')).toEqual([
    { type: 'code_new', workspace: 'recipe-box' },
    { type: 'code_new', workspace: 'recipe-box' },
  ]);
  await expect(page.getByTestId('workspace-chip')).toHaveText('recipe-box');
  // Once it's made (the PC's `chat`), it's an ordinary Code chat: not told again.
  await page.getByTestId('input').fill('Explain how this project is laid out');
  await page.getByTestId('input').press('Enter');
  pc.send({ type: 'chat', id: 15, title: 'Project layout' });
  await expect.poll(() => page.evaluate(() => localStorage.getItem('yap.link.codeNew'))).toBe(null);
});

test('Remote Control off on the PC: the page is Chat, as before', async ({ page }) => {
  const pc = await mockCodePc(page, { ready: { on: false, why: 'Remote Control is off on Office PC. Turn it on in Settings → Phone.' } });
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await expect(drawer(page)).toBeVisible();
  await expect(page.getByTestId('sides')).toBeHidden();
  await expect(page.getByTestId('code-side')).toBeHidden();
  await expect(page.getByTestId('new-chat')).toHaveText('New chat');
  await page.getByTestId('recent').filter({ hasText: 'Watering schedule' }).click();
  await expect(page.getByTestId('turn')).toHaveCount(2);
  // A report is a quiet note, as before; no pills, no chips, no cards.
  await expect(page.getByTestId('report')).toHaveText("The agent in garden-planner: it's done.");
  await expect(page.getByTestId('report-card')).toHaveCount(0);
  await expect(page.getByTestId('agent-pill')).toHaveCount(0);
  await expect(page.getByTestId('coderow')).toBeHidden();
  await expect(page.getByTestId('needs')).toBeHidden();
  // Nothing of the Code side asked for.
  expect(pc.types().filter((t) => t.startsWith('code_') || t.startsWith('agent_'))).toEqual([]);
  await expect(page.getByTestId('notify-row')).toBeHidden();
});

test('a PC from before Remote Control: Chat, as before', async ({ page }) => {
  const pc = await mockCodePc(page, { can: CAN, ready: null });
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await expect(page.getByTestId('sides')).toBeHidden();
  expect((await snapshot(page)).code.on).toBe(false);
  expect(pc.types().filter((t) => t.startsWith('code_'))).toEqual([]);
});

test('Remote Control turned off while a Code chat is open: the chips go and the page says why', async ({ page }) => {
  const pc = await mockCodePc(page);
  await open(page);
  await ready(page, pc);
  await page.getByTestId('menu').click();
  await page.getByTestId('side-code').click();
  await page.locator('[data-testid="code-chat"][data-id="12"]').click();
  await expect(page.getByTestId('workspace-chip')).toHaveText('garden-planner');
  await expect(page.getByTestId('report-card')).toHaveCount(1);
  pc.code = { on: false, why: 'Remote Control is off on Office PC. Turn it on in Settings → Phone.' };
  pc.ready();
  await expect(page.getByTestId('coderow')).toBeHidden();
  await expect(page.getByTestId('note')).toHaveText('Remote Control is off on Office PC. Turn it on in Settings → Phone.');
  await expect(page.getByTestId('report-card')).toHaveCount(0);
  await expect(page.getByTestId('report')).toHaveCount(1);
  await page.getByTestId('menu').click();
  await expect(page.getByTestId('sides')).toBeHidden();
  void out;
  void AGENT_ID;
});
