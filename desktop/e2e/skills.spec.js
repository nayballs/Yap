// Yap's built-in Skills (desktop/src-tauri/src/chat/skills.rs; Wispr Flow's
// Windows build has 24), on a new install:
// - the core six are in the "/" menus where they run — a meeting's Ask box
//   /followup-email, /decisions and /summary-for; Chat /prep, /standup,
//   /todos, /decisions and /summary-for — and your own above them; the
//   other nine wait in Settings → Skills' More Skills;
// - one of those typed in full and sent still runs: /open-questions on a
//   meeting in the Notes view's Ask bar cites its lines; /decision-log in
//   Chat reads each meeting's decisions, oldest first, and cites them;
// - /prd in Chat writes a Chat document, its "[1]"s named by their meeting,
//   and the line in the chat carries no "couldn't tie" note; /tickets on
//   one meeting leaves them out (the document is that meeting's);
// - Settings: a switch puts /project-update in the menus (Enter there puts
//   it in the box, as it needs a project; sent alone Yap asks which, with
//   no model call), and off again takes it out; the menu's More Skills
//   opens Settings there; light and dark;
// - Settings → Skills lists them, says which write a document in Chat, and
//   copies one as your own, which the menu then lists first.
// The AI is the local fake (support/fake-llm.js), which records every
// request; the meetings are fictional, seeded in notes.json.
// (skills-existing.spec.js: an install from before the core six.)
import { test as base, expect, openView, openSettings, closeSettings, closeToasts, setConfig, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { needsWords, writesDocument, matchSkills, skillMenu as menuOf, skillGroups, promptChips, skillFor } from '../src/lib/core/skills.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

const NOTES = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Supplier call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 3 * DAY,
      updatedTs: NOW - 3 * DAY,
      participants: ['Priya Shah'],
      enhancedContent:
        '## Recap\nThe packaging quote came in over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n\n## Decisions\n- Ask finance for the extra budget, because the quote is final\n\n## Open questions\n- Can the supplier deliver before the launch?',
      transcript: [
        { source: 'you', text: 'Priya, is the quote final now?', ts: NOW - 3 * DAY },
        { source: 'them', text: 'Yes, it is final. Can the supplier even deliver before the launch?', ts: NOW - 3 * DAY + 20 },
        { source: 'you', text: 'Nobody knows yet, we should find out.', ts: NOW - 3 * DAY + 40 },
      ],
    },
    {
      id: 2,
      title: 'Offsite planning',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 10 * DAY,
      updatedTs: NOW - 10 * DAY,
      participants: ['Alice Moreno'],
      enhancedContent: '## Recap\nThe team planned the spring offsite.\n\n## Decisions\n- Hold the offsite in Lisbon, because flights are cheaper',
      transcript: [
        { source: 'you', text: 'Berlin or Lisbon for the offsite?', ts: NOW - 10 * DAY },
        { source: 'them', text: 'Lisbon, the flights are cheaper for most of us.', ts: NOW - 10 * DAY + 15 },
      ],
    },
  ],
};

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
      await use({
        name: 'skills',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
          },
          meetingReminder: 'never',
          // A new install's config: no one-time migration to run.
          configVersion: 7,
        },
        data: { 'notes.json': NOTES },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const skillMenu = (page) => page.getByRole('listbox', { name: 'Skills' });
const panel = (main) => main.getByTestId('chat-doc');
/** Every message of a request, as one text. */
const sentText = (r) => (r?.messages ?? []).map((m) => (typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? ''))).join('\n');

/** The document `create_document` made earlier in this turn (its result in the messages). */
function madeDoc(record) {
  for (const m of [...record.messages].reverse()) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (Array.isArray(r?.data?.sections)) return r.data.id;
    } catch {
      /* not JSON */
    }
  }
  return null;
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
}

test.afterEach(async ({ main, fakeLlm }) => {
  fakeLlm.reply('other', undefined);
  await closeToasts(main);
});

test('a Skill that needs words goes in the box, and the document ones say so', () => {
  const list = [
    { id: 'builtin:prep', command: 'prep', name: 'Prepare me', runsOn: 'meetings', builtin: true },
    { id: 'builtin:project-update', command: 'project-update', name: 'Project update', runsOn: 'meetings', builtin: true, asks: 'Which project?' },
    { id: 'builtin:prd', command: 'prd', name: 'Product requirements', runsOn: 'both', builtin: true, document: 'the sections…' },
  ];
  expect(needsWords(list[1])).toBe(true);
  expect(needsWords(list[0])).toBe(false);
  expect(writesDocument(list[2])).toBe(true);
  expect(writesDocument(list[0])).toBe(false);
  expect(matchSkills(list, 'p', 'meeting').map((s) => s.command)).toEqual(['prd']);
  expect(matchSkills(list, 'p', 'meetings').map((s) => s.command)).toEqual(['prep', 'project-update', 'prd']);
});

test('the menus list the Skills shown: yours then Yap’s, the best match first; the prompts are the first three', () => {
  const list = [
    { id: 'builtin:prep', command: 'prep', name: 'Prepare me', runsOn: 'meetings', builtin: true, core: true, shown: true, gather: 'prep', chip: 'Prepare for my next meeting', question: 'Prepare me for my next meeting.' },
    { id: 'builtin:standup', command: 'standup', name: 'Stand-up', runsOn: 'meetings', builtin: true, core: true, shown: true, chip: 'Write my stand-up', question: 'Write my stand-up update.' },
    { id: 'builtin:todos', command: 'todos', name: 'My open to-dos', runsOn: 'meetings', builtin: true, core: true, shown: true, chip: 'List my open to-dos', question: 'List my open to-dos.' },
    { id: 'builtin:decisions', command: 'decisions', name: 'Decisions', runsOn: 'both', builtin: true, core: true, shown: true, chip: 'What did we decide?', question: 'What did we decide in my last meeting?' },
    { id: 'builtin:decision-log', command: 'decision-log', name: 'Decision log', runsOn: 'meetings', builtin: true, core: false, shown: false },
    { id: 'builtin:tickets', command: 'tickets', name: 'Draft tickets', runsOn: 'both', builtin: true, core: false, shown: false },
    { id: 's1', command: 'my-decisions', name: 'My decisions', runsOn: 'both', shown: true },
  ];
  // Just "/": yours, then Yap's shown; never one switched off.
  const browse = menuOf(list, '', 'meetings');
  expect(browse.groups.map((g) => [g.label, g.start, g.items.map((s) => s.command)])).toEqual([
    ['Your Skills', 0, ['my-decisions']],
    ["Yap's Skills", 1, ['prep', 'standup', 'todos', 'decisions']],
  ]);
  expect(browse.items.map((s) => s.command)).toEqual(['my-decisions', 'prep', 'standup', 'todos', 'decisions']);
  // A command begun: one list, what was typed first (Enter runs it), then
  // the ones starting with it, then the ones holding it.
  const typed = menuOf(list, 'decisions', 'meetings');
  expect(typed.groups.map((g) => g.label)).toEqual(['']);
  expect(typed.items.map((s) => s.command)).toEqual(['decisions', 'my-decisions']);
  expect(menuOf(list, 'tick', 'meetings').items).toEqual([]);
  // The whole command of one off the menu: no menu, so Enter sends it as typed
  // (never yours that holds the words).
  const withYours = [...list, { id: 's2', command: 'my-tickets', name: 'My tickets', runsOn: 'both', shown: true }];
  expect(menuOf(withYours, 'ticket', 'meetings').items.map((s) => s.command)).toEqual(['my-tickets']);
  expect(menuOf(withYours, 'tickets', 'meetings').items).toEqual([]);
  // Off the menu, typed in full and sent: it runs.
  expect(skillFor(list, 'tickets', 'meetings').name).toBe('Draft tickets');
  // Settings' groups.
  const g = skillGroups(list);
  expect([g.mine, g.yaps, g.more].map((x) => x.map((s) => s.command))).toEqual([['my-decisions'], ['prep', 'standup', 'todos', 'decisions'], ['decision-log', 'tickets']]);
  // The prompts: three at most; /prep's only with a calendar.
  expect(promptChips(list, { calendar: true }).map((s) => s.chip)).toEqual(['Prepare for my next meeting', 'Write my stand-up', 'List my open to-dos']);
  expect(promptChips(list, { calendar: false }).map((s) => s.chip)).toEqual(['Write my stand-up', 'List my open to-dos', 'What did we decide?']);
  // Switched off: gone from the prompts too.
  const off = list.map((s) => (s.command === 'standup' ? { ...s, shown: false } : s));
  expect(promptChips(off, { calendar: false }).map((s) => s.chip)).toEqual(['List my open to-dos', 'What did we decide?']);
});

test('"/" lists the core six where they run; one off the menu typed in full runs, on a meeting too', async ({ main, fakeLlm, shot }) => {
  // Chat: the core Skills that run across the meetings, nothing else.
  await freshChat(main);
  await box(main).fill('/');
  const menu = skillMenu(main);
  await expect(menu.getByRole('option')).toHaveText([/^\/prep/, /^\/standup/, /^\/todos/, /^\/decisions/, /^\/summary-for/]);
  await expect(menu.getByRole('group', { name: "Yap's Skills" })).toBeVisible();
  await expect(menu).toContainText('Get ready for your next meeting');
  for (const c of ['/followup-email', '/tickets', '/decision-log', '/project-update']) await expect(menu).not.toContainText(c);
  await shot(main, '01-chat-skill-menu');
  // ↓ moves the highlight, the box says which (aria-activedescendant).
  await box(main).press('ArrowDown');
  await expect(box(main)).toHaveAttribute('aria-activedescendant', 'skill-opt-1');
  await expect(menu.getByRole('option', { selected: true })).toHaveText(/^\/standup/);
  await box(main).press('ArrowUp');
  await box(main).press('ArrowUp');
  await expect(menu.getByRole('option', { selected: true })).toHaveText(/^\/summary-for/);
  // Begun: the matches, without the headings.
  await box(main).fill('/st');
  await expect(menu.getByRole('option')).toHaveText([/^\/standup/]);
  await expect(menu.getByRole('group', { name: "Yap's Skills" })).toHaveCount(0);
  // One that's off: no menu for it.
  await box(main).fill('/tick');
  await expect(menu).toHaveCount(0);
  await box(main).fill('');

  // A meeting note's Ask bar in Notes: the three of the core that run on one meeting.
  await openView(main, 'Notes');
  await main.getByRole('complementary').getByRole('button', { name: /^Meetings\b/ }).first().click();
  await main.locator('.items .item').filter({ hasText: 'Supplier call' }).click();
  const ask = main.getByPlaceholder('Ask anything…');
  await ask.fill('/');
  await expect(skillMenu(main).getByRole('option')).toHaveText([/^\/followup-email/, /^\/decisions/, /^\/summary-for/]);
  await expect(skillMenu(main)).not.toContainText('/standup');
  await expect(skillMenu(main)).not.toContainText('/open-questions');
  // /open-questions is off the menu: typed in full and sent, it runs.
  await ask.fill('/open-questions');
  await expect(skillMenu(main)).toHaveCount(0);
  await ask.press('Enter');
  const answer = main.locator('.chatthread .cbubble.assistant').last();
  await expect(answer.getByRole('heading', { name: 'Open questions' })).toBeVisible();
  await expect(answer.locator('a.md-passage').first()).toBeVisible();
  const run = fakeLlm.requests.filter((r) => r.kind === 'skill').at(-1);
  // On the meeting's model, told which it is (ai_model::answering_line).
  expect(run.system).toContain("You are Yap's assistant, running on Fake Notes, on this PC.");
  expect(run.user).toContain('<passage n="1"');
  expect(run.user).toContain('Can the supplier even deliver before the launch?');
  // The questions asked, in order, for the model to go through (skills::meeting_aid).
  expect(run.user).toContain('The questions asked in it, in order, each with what was said next');
  expect(run.user).toContain('Q2. Them: "Can the supplier even deliver before the launch?"\n  Said next: You: "Nobody knows yet, we should find out."');
  expect(run.user).toMatch(/The Skill to carry out on this meeting: Open questions \(\/open-questions\)\nInstructions:\nList only the questions that never got an answer/);
  await shot(main, '02-open-questions-on-a-meeting');
});

test('/decision-log in Chat, off the menu and typed in full, reads each meeting’s decisions, oldest first, and cites them', async ({ main, fakeLlm, shot }) => {
  fakeLlm.reply('other', () => '- **Offsite planning:** Hold the offsite in Lisbon, because flights are cheaper [1].\n- **Supplier call:** Ask finance for the extra budget [2].');
  await freshChat(main);
  const before = fakeLlm.requests.length;
  await box(main).fill('/decision-log');
  await expect(skillMenu(main)).toHaveCount(0);
  await box(main).press('Enter');
  await expect(main.locator('.cbubble.user').last()).toContainText('/decision-log');
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer).toContainText('Hold the offsite in Lisbon');
  await expect(answer.locator('a.md-passage')).toHaveCount(2);
  await expect(main.locator('.toolchip').filter({ hasText: /Ran \/decision-log · 2 passages from the last 4 weeks/ })).toBeAttached();
  const req = fakeLlm.requests.slice(before).filter((r) => r.kind === 'other').at(-1);
  const sent = sentText(req) + req.system;
  expect(sent).toContain('Skill: Decision log (/decision-log)');
  expect(sent).toContain('The passages are the decisions recorded in each of their meetings then, oldest first');
  // Oldest first: the offsite before the supplier call.
  expect(sent.indexOf('Hold the offsite in Lisbon')).toBeGreaterThan(-1);
  expect(sent.indexOf('Hold the offsite in Lisbon')).toBeLessThan(sent.indexOf('Ask finance for the extra budget'));
  await shot(main, '03-decision-log');
});

test('/prd in Chat writes a document, its citations named by their meeting', async ({ main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: {
            title: 'Requirements: supplier delivery',
            summary: 'From the supplier call.',
            sections: [
              { key: 'Problem', intent: 'who has it and why it matters' },
              { key: 'Requirements', intent: 'the must-haves' },
            ],
          },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Problem', markdown: 'The packaging quote came in over budget [1, 2].' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Requirements', markdown: '- Delivery before the launch [1, 9].' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  const before = fakeLlm.requests.length;
  // Words that name no meeting: a search for them, over both meetings.
  await box(main).fill('/prd supplier and offsite');
  await box(main).press('Enter');
  await expect(panel(main)).toBeVisible();
  await expect(panel(main).locator('.doctitle')).toHaveText('Requirements: supplier delivery');
  const body = panel(main).locator('.docbody');
  await expect(body).toContainText('Delivery before the launch', { timeout: 20_000 });
  // "[1]" is its meeting by name; a number that's none of the passages goes.
  await expect(body).toContainText(/over budget \((Supplier call|Offsite planning), /);
  await expect(body).not.toContainText('[1');
  await expect(body).not.toContainText('9]');
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer).toContainText("It's open on the right.");
  await expect(answer).not.toContainText("couldn't tie");
  // The Skill asked for a document, with the passages the search found.
  const asked = fakeLlm.requests.slice(before).find((r) => r.tools.includes('create_document'));
  expect(sentText(asked)).toContain('Write this as a document now, not in the chat: create_document');
  expect(sentText(asked)).toContain('The passages are what their meetings and notes say about "supplier and offsite".');
  await shot(main, '04-prd-document');
});

test('a document from one meeting leaves its citations out: the document is that meeting’s', async ({ main, fakeLlm }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_document', args: { title: 'Tickets: Supplier call', sections: [{ key: 'Draft the finance request', intent: 'the ticket' }] } }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Draft the finance request', markdown: '**What:** Ask finance for the extra budget [3].\n**Owner:** You [2]' }) }] },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await box(main).fill('/tickets the supplier call');
  await box(main).press('Enter');
  const body = panel(main).locator('.docbody');
  await expect(body).toContainText('Ask finance for the extra budget', { timeout: 20_000 });
  await expect(body).not.toContainText('Supplier call,');
  await expect(body).not.toContainText('[3]');
  await expect(main.locator('.toolchip').filter({ hasText: /Ran \/tickets · \d+ passages in "Supplier call"/ })).toBeAttached();
});

/** Settings → Skills' switch for `/command`. */
const menuSwitch = (dialog, command) => dialog.getByRole('button', { name: `Show /${command} in the / menu` });

test('Settings: a switch puts /project-update in the menus — Enter puts it in the box, alone Yap asks which — and off takes it out', async ({ yap, main, fakeLlm, shot }) => {
  const dialog = await openSettings(main, 'Skills');
  // Yap's core six on, the other nine in More Skills, off.
  const yaps = dialog.getByRole('group', { name: "Yap's Skills" });
  const more = dialog.getByRole('group', { name: 'More Skills' });
  await expect(yaps.locator('[data-skill]')).toHaveCount(6);
  await expect(more.locator('[data-skill]')).toHaveCount(9);
  for (const c of ['followup-email', 'prep', 'standup', 'todos', 'decisions', 'summary-for']) await expect(menuSwitch(yaps, c)).toHaveAttribute('aria-pressed', 'true');
  for (const c of ['decision-log', 'open-questions', 'risks', 'tickets', 'prd', 'coach', 'one-to-one', 'weekly-recap', 'project-update']) {
    await expect(menuSwitch(more, c)).toHaveAttribute('aria-pressed', 'false');
  }
  await expect(more.locator('[data-skill="tickets"]')).toContainText('Draft tickets for the work agreed');
  // On: saved at once, every menu follows.
  await menuSwitch(more, 'project-update').click();
  await expect(menuSwitch(more, 'project-update')).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'skills.json', (s) => s.menu?.['builtin:project-update'] === true, 'the switch to be saved');
  await closeSettings(main);

  await freshChat(main);
  await box(main).fill('/');
  await expect(skillMenu(main).getByRole('option')).toHaveText([/^\/prep/, /^\/standup/, /^\/todos/, /^\/decisions/, /^\/summary-for/, /^\/project-update/]);
  // It needs a project: Enter puts it in the box for one…
  await box(main).fill('/project');
  await box(main).press('Enter');
  await expect(box(main)).toHaveValue('/project-update ');
  const before = fakeLlm.requests.filter((r) => r.kind === 'other').length;
  // …and sent alone, Yap asks which.
  await box(main).press('Enter');
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer).toContainText('Which project? Add its name after the command, like /project-update website relaunch.');
  // No model was asked.
  expect(fakeLlm.requests.filter((r) => r.kind === 'other').length).toBe(before);

  // Off again: out of the menus.
  await openSettings(main, 'Skills');
  await menuSwitch(more, 'project-update').click();
  await expectStore(yap, 'skills.json', (s) => s.menu?.['builtin:project-update'] === false, 'the switch to be saved');
  await closeSettings(main);
  await box(main).fill('/project');
  await expect(skillMenu(main)).toHaveCount(0);
  await box(main).fill('');
});

test('the menu’s More Skills opens Settings there, and one switched on is in the menu on the way back; light and dark', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  await freshChat(main);
  await box(main).fill('/');
  await main.getByTestId('skill-menu').getByRole('button', { name: 'More Skills' }).click();
  const dialog = main.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.getByRole('heading', { level: 1, name: 'Skills' })).toBeVisible();
  const more = dialog.getByRole('group', { name: 'More Skills' });
  await expect(more).toBeInViewport();
  await menuSwitch(more, 'tickets').click();
  await expect(menuSwitch(more, 'tickets')).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '06-skills-settings-more-light');
  await dialog.getByRole('group', { name: "Yap's Skills" }).locator('[data-skill="prep"]').getByRole('button', { name: 'Open' }).click();
  await dialog.locator('#settings-skills-yours').scrollIntoViewIfNeeded();
  await shot(main, '07-skills-settings-light');
  await closeSettings(main);
  // The menu stayed open, /tickets in it now, after the core.
  await expect(skillMenu(main).getByRole('option')).toHaveText([/^\/prep/, /^\/standup/, /^\/todos/, /^\/decisions/, /^\/summary-for/, /^\/tickets/]);
  await shot(main, '08-chat-skill-menu-light');

  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  try {
    await box(main).fill('');
    await box(main).fill('/');
    await expect(skillMenu(main)).toBeVisible();
    await shot(main, '09-chat-skill-menu-dark');
    await box(main).fill('');
    const again = await openSettings(main, 'Skills');
    await again.locator('#settings-skills-yours').scrollIntoViewIfNeeded();
    await shot(main, '10-skills-settings-dark');
    await again.getByRole('group', { name: 'More Skills' }).scrollIntoViewIfNeeded();
    await shot(main, '11-skills-settings-more-dark');
  } finally {
    // Back as it was for the next test: /tickets off, light.
    const s = await openSettings(main, 'Skills');
    await menuSwitch(s.getByRole('group', { name: 'More Skills' }), 'tickets').click();
    await expectStore(yap, 'skills.json', (j) => j.menu?.['builtin:tickets'] === false, 'the switch to be saved');
    await closeSettings(main);
    await setConfig(yap, main, { theme: 'light' });
    await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('light');
  }
});

test('Settings → Skills lists them, says which write a document, and copies one, which the menu lists first', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Skills');
  for (const c of ['summary-for', 'decision-log', 'open-questions', 'tickets', 'prd', 'standup', 'prep', 'one-to-one', 'project-update']) {
    await expect(dialog.locator(`[data-skill="${c}"]`)).toBeVisible();
  }
  const prd = dialog.locator('[data-skill="prd"]');
  await prd.getByRole('button', { name: 'Open' }).click();
  await expect(prd).toContainText('it writes its answer as a document beside the chat');
  await prd.getByRole('button', { name: 'Close' }).click();
  const standup = dialog.locator('[data-skill="standup"]');
  await standup.getByRole('button', { name: 'Open' }).click();
  await expect(standup).toContainText('stand-up update in three parts');
  await expect(standup).not.toContainText('a document beside the chat');
  await shot(main, '05-skills-settings');
  const prep = dialog.locator('[data-skill="prep"]');
  await prep.getByRole('button', { name: 'Open' }).click();
  await prep.getByRole('button', { name: 'Save a copy' }).click();
  const form = dialog.getByRole('group', { name: 'New Skill' });
  await expect(form).toContainText("Based on Yap's /prep.");
  await expect(form.getByRole('textbox', { name: 'Skill command' })).toHaveValue('my-prep');
  await form.getByRole('button', { name: 'Save Skill' }).click();
  const mine = dialog.getByRole('group', { name: 'Your Skills' });
  await expect(mine.locator('[data-skill="my-prep"]')).toBeVisible();
  // Yours: in the menus to begin with.
  await expect(menuSwitch(mine, 'my-prep')).toHaveAttribute('aria-pressed', 'true');
  const stored = yap.readJson('skills.json');
  expect(stored.skills.map((s) => [s.command, s.basedOn ?? ''])).toEqual([['my-prep', 'prep']]);
  // What only Yap's own carry never comes with a copy.
  expect(stored.skills[0].gather).toBeUndefined();
  await closeSettings(main);

  // In Chat's menu: yours first, then Yap's.
  await freshChat(main);
  await box(main).fill('/');
  const menu = skillMenu(main);
  await expect(menu.getByRole('group', { name: 'Your Skills' }).getByRole('option')).toHaveText([/^\/my-prep/]);
  await expect(menu.getByRole('option')).toHaveText([/^\/my-prep/, /^\/prep/, /^\/standup/, /^\/todos/, /^\/decisions/, /^\/summary-for/]);
  await shot(main, '12-chat-skill-menu-yours');
  // "/prep" typed: Yap's /prep first, the one Enter runs; yours after it.
  await box(main).fill('/prep');
  await expect(menu.getByRole('option')).toHaveText([/^\/prep/, /^\/my-prep.*Yours$/]);
  // Switched off in Settings: out of the menu.
  await openSettings(main, 'Skills');
  await menuSwitch(dialog.getByRole('group', { name: 'Your Skills' }), 'my-prep').click();
  await expectStore(yap, 'skills.json', (s) => s.menu?.[stored.skills[0].id] === false, 'the switch to be saved');
  await closeSettings(main);
  await box(main).fill('/my-');
  await expect(skillMenu(main)).toHaveCount(0);
  await box(main).fill('');
});
