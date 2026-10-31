// Yap's newer built-in Skills (src-tauri/src/chat/skills.rs, 31 Oct 2026;
// Wispr Flow's Windows build has 24):
// - "/" lists them where they run: a meeting's Ask box the eight that run on
//   one meeting (/summary-for, /open-questions, /tickets, /prd among them),
//   Chat the ones that run across the meetings (/prep, /standup,
//   /one-to-one, /decision-log, /project-update…);
// - /open-questions on a meeting in the Notes view's Ask bar cites its lines;
// - /decision-log in Chat reads each meeting's decisions, oldest first, and
//   its answer cites them;
// - /prd in Chat writes a Chat document, its "[1]"s named by their meeting,
//   and the line in the chat carries no "couldn't tie" note; /tickets on
//   one meeting leaves them out (the document is that meeting's);
// - /project-update needs a project: Enter in the menu puts it in the box,
//   and sent alone Yap asks which, with no model call;
// - Settings → Skills lists them, says which write a document in Chat, and
//   copies one as your own.
// The AI is the local fake (support/fake-llm.js), which records every
// request; the meetings are fictional, seeded in notes.json.
import { test as base, expect, openView, openSettings, closeSettings, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { needsWords, writesDocument, matchSkills } from '../src/lib/core/skills.js';

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

test('"/" lists the new Skills where they run; /open-questions on a meeting cites its lines', async ({ main, fakeLlm, shot }) => {
  // Chat: the ones that run across the meetings.
  await freshChat(main);
  await box(main).fill('/');
  const menu = skillMenu(main);
  for (const c of ['/prep', '/standup', '/one-to-one', '/decision-log', '/project-update', '/tickets', '/prd', '/summary-for', '/open-questions']) {
    await expect(menu).toContainText(c);
  }
  await expect(menu).not.toContainText('/followup-email');
  await expect(menu.getByRole('option')).toHaveCount(14);
  await shot(main, '01-chat-skill-menu');
  await box(main).fill('');

  // A meeting note's Ask bar in Notes: the eight that run on one meeting.
  await openView(main, 'Notes');
  await main.getByRole('complementary').getByRole('button', { name: /^Meetings\b/ }).first().click();
  await main.locator('.items .item').filter({ hasText: 'Supplier call' }).click();
  const ask = main.getByPlaceholder('Ask anything…');
  await ask.fill('/');
  await expect(skillMenu(main).getByRole('option')).toHaveCount(8);
  for (const c of ['/summary-for', '/open-questions', '/tickets', '/prd']) await expect(skillMenu(main)).toContainText(c);
  await expect(skillMenu(main)).not.toContainText('/standup');
  await expect(skillMenu(main)).not.toContainText('/prep');
  await ask.fill('/open');
  await expect(skillMenu(main).getByRole('option')).toHaveCount(1);
  await ask.press('Enter');
  const answer = main.locator('.chatthread .cbubble.assistant').last();
  await expect(answer.getByRole('heading', { name: 'Open questions' })).toBeVisible();
  await expect(answer.locator('a.md-passage').first()).toBeVisible();
  const run = fakeLlm.requests.filter((r) => r.kind === 'skill').at(-1);
  expect(run.user).toContain('<passage n="1"');
  expect(run.user).toContain('Can the supplier even deliver before the launch?');
  // The questions asked, in order, for the model to go through (skills::meeting_aid).
  expect(run.user).toContain('The questions asked in it, in order, each with what was said next');
  expect(run.user).toContain('Q2. Them: "Can the supplier even deliver before the launch?"\n  Said next: You: "Nobody knows yet, we should find out."');
  expect(run.user).toMatch(/The Skill to carry out on this meeting: Open questions \(\/open-questions\)\nInstructions:\nList only the questions that never got an answer/);
  await shot(main, '02-open-questions-on-a-meeting');
});

test('/decision-log in Chat reads each meeting’s decisions, oldest first, and cites them', async ({ main, fakeLlm, shot }) => {
  fakeLlm.reply('other', () => '- **Offsite planning:** Hold the offsite in Lisbon, because flights are cheaper [1].\n- **Supplier call:** Ask finance for the extra budget [2].');
  await freshChat(main);
  const before = fakeLlm.requests.length;
  await box(main).fill('/decision-');
  await expect(skillMenu(main).getByRole('option')).toHaveText([/\/decision-log/]);
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

test('/project-update needs a project: Enter puts it in the box, and alone Yap asks which', async ({ main, fakeLlm }) => {
  await freshChat(main);
  await box(main).fill('/project');
  await box(main).press('Enter');
  await expect(box(main)).toHaveValue('/project-update ');
  const before = fakeLlm.requests.filter((r) => r.kind === 'other').length;
  await box(main).press('Enter');
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer).toContainText('Which project? Add its name after the command, like /project-update website relaunch.');
  // No model was asked.
  expect(fakeLlm.requests.filter((r) => r.kind === 'other').length).toBe(before);
});

test('Settings → Skills lists the new ones, says which write a document, and copies one', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Skills');
  for (const c of ['summary-for', 'decision-log', 'open-questions', 'tickets', 'prd', 'standup', 'prep', 'one-to-one', 'project-update']) {
    await expect(dialog.locator(`[data-skill="${c}"]`)).toBeVisible();
  }
  await expect(dialog.locator('[data-skill="prd"]')).toContainText('a document in Chat');
  await expect(dialog.locator('[data-skill="prep"]')).not.toContainText('a document in Chat');
  const standup = dialog.locator('[data-skill="standup"]');
  await standup.getByRole('button', { name: 'Open' }).click();
  await expect(standup).toContainText("stand-up update in three parts");
  await shot(main, '05-skills-settings');
  const prep = dialog.locator('[data-skill="prep"]');
  await prep.getByRole('button', { name: 'Save a copy' }).click();
  const form = dialog.getByRole('group', { name: 'New Skill' });
  await expect(form).toContainText("Based on Yap's /prep.");
  await expect(form.getByRole('textbox', { name: 'Skill command' })).toHaveValue('my-prep');
  await form.getByRole('button', { name: 'Save Skill' }).click();
  await expect(dialog.locator('[data-skill="my-prep"]')).toBeVisible();
  const stored = yap.readJson('skills.json');
  expect(stored.skills.map((s) => [s.command, s.basedOn ?? ''])).toEqual([['my-prep', 'prep']]);
  // What only Yap's own carry never comes with a copy.
  expect(stored.skills[0].gather).toBeUndefined();
  await closeSettings(main);
});
