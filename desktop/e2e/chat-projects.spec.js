// Projects (desktop/src-tauri/src/chat/projects.rs; views/chat/ProjectsView.svelte, chatState's
// `page`; 15 Nov 2026, the Claude app's projects from Anthropic's help pages): a named group of
// chats sharing instructions and files that every chat in it gets.
//
// - Projects in the conversation list opens the page; New project makes one (a name, what it's
//   for) and opens it. Its instructions save as typed; a file added is read with the attachments'
//   rules (a Markdown file kept, a spreadsheet said plainly and left out).
// - New chat in it: the band says so; the question's system prompt carries the project's name,
//   instructions and the file's text; the chat is listed on the project's page.
// - A chat's ⋯ → Move to project ▸ puts another chat in; Remove from project takes it out.
// - Deleted (asked first): its chats stay, out of it. Light and dark.
// - Nothing in chats.json: projects.json, a file an older Yap never touches.
// - **A big project** (chat/projects/knowledge.rs, 16 Nov 2026, the Claude app's project
//   knowledge): files too long for the model (the fake's 8K) aren't sent whole. The system
//   prompt lists them and their parts; the question gets the part that matches it, its
//   <source> naming the file and its section; the model reads another with
//   read_project_file; the answer's steps say which parts were used; the project's page says
//   how the files are read.
//
// The model is the scripted fake (support/fake-llm.js). Every detail is fictional.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { chipKind, stepsSummary } from '../src/lib/views/chat/chatSteps.js';
import { knowledgeLine } from '../src/lib/views/chat/projectLines.js';

const HERE = path.join(RUNS, 'chat-projects-spec');
const SOWING = path.join(HERE, 'sowing-plan.md');
const SHEET = path.join(HERE, 'seed-costs.xlsx');
const HANDBOOK = path.join(HERE, 'staff-handbook.md');

/** A fictional staff handbook in Markdown, far too long for an 8K model: 40 sections, one the leave policy. */
function handbook() {
  const topics = ['Working hours', 'Expenses', 'Annual leave', 'Sick pay', 'Parking', 'Training', 'Security', 'Equipment', 'Travel', 'Pensions'];
  let s = '# Example Ltd staff handbook\n\n';
  for (let i = 0; i < 40; i++) {
    const topic = topics[i % topics.length];
    s += `## ${i + 1}. ${topic}${i >= topics.length ? ` (${Math.floor(i / topics.length) + 1})` : ''}\n\n`;
    let para = '';
    while (para.split(/\s+/).length < 300) para += `The ${topic.toLowerCase()} policy at Example Ltd covers every member of staff, and line managers check it each quarter. `;
    s += para;
    if (i === 2) s += 'Full-time staff get 27 days of annual leave a year plus bank holidays, and up to five unused days carry over.';
    s += '\n\n';
  }
  return s;
}

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
      fs.rmSync(HERE, { recursive: true, force: true });
      fs.mkdirSync(HERE, { recursive: true });
      fs.writeFileSync(SOWING, '# Sowing plan\n\nBroad beans go in on 3 March, in bed 4.\n');
      fs.writeFileSync(SHEET, Buffer.from([0x50, 0x4b, 0x03, 0x04, 0, 0, 0, 0]));
      fs.writeFileSync(HANDBOOK, handbook());
      await use({
        name: 'chat-projects',
        config: {
          postProcessEnabled: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
        },
        data: {
          'chats.json': {
            conversations: [
              { id: 1, title: 'Greenhouse heater', createdTs: 1, updatedTs: Math.floor(Date.now() / 1000) - 60, messages: [{ role: 'user', text: 'Which heater?', ts: 1 }, { role: 'assistant', text: 'A small paraffin one.', ts: 2 }] },
            ],
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const page = (main) => main.getByTestId('chat-project');
const askedWith = (fakeLlm, text) => fakeLlm.requests.filter((r) => Array.isArray(r.messages) && JSON.stringify(r.messages).includes(text) && r.kind === 'other').at(-1);
const systemOf = (req) => String(req?.messages?.find((m) => m.role === 'system')?.content ?? '');

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

test('a project: made, its instructions and files, a chat in it carrying them, and deleted', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await expect(main.getByTestId('chat-projects')).toContainText('No projects yet.');
  await main.getByRole('button', { name: 'New project' }).click();
  await main.getByRole('textbox', { name: 'Name' }).fill('Allotment 2027');
  await main.getByRole('textbox', { name: /What it's for/ }).fill('Planning the plot for next year');
  await main.getByRole('button', { name: 'Create project' }).click();
  await expect(page(main).getByRole('heading', { name: 'Allotment 2027' })).toBeVisible();

  // Instructions save as typed.
  await page(main).getByRole('textbox', { name: 'Instructions' }).fill('Use metric units. Bed numbers are on the plot map.');
  await expect(page(main).getByText('Saved')).toBeVisible();
  const [p] = yap.readJson('projects.json').projects;
  expect(p.instructions).toBe('Use metric units. Bed numbers are on the plot map.');

  // Files, read as attachments are: the Markdown kept, the spreadsheet said and left out.
  const out = await yap.invoke('project_files_add', { id: p.id, paths: [SOWING, SHEET] });
  expect(out.added.map((f) => f.name)).toEqual(['sowing-plan.md']);
  expect(out.skipped.map((s) => s.name)).toEqual(['seed-costs.xlsx']);
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByRole('button', { name: /Allotment 2027/ }).click();
  await expect(page(main).getByRole('list', { name: 'Files in Allotment 2027' })).toContainText('sowing-plan.md');
  await shot(main, '01-project-page');
  await theme(yap, main, 'dark');
  await shot(main, '02-project-page-dark');
  await theme(yap, main, 'light');

  // A chat in it: the band, then its question carries the project.
  await page(main).getByRole('button', { name: 'New chat in Allotment 2027' }).click();
  await expect(main.getByTestId('chat-project-line')).toContainText('Allotment 2027');
  fakeLlm.scriptChat([{ content: 'On 3 March, in bed 4.' }]);
  await box(main).fill('When do the broad beans go in?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('bed 4');
  const sys = systemOf(askedWith(fakeLlm, 'When do the broad beans go in?'));
  expect(sys).toContain('This chat is in the user\'s project "Allotment 2027". It\'s for: Planning the plot for next year');
  expect(sys).toContain('<project_instructions>\nUse metric units. Bed numbers are on the plot map.\n</project_instructions>');
  expect(sys).toContain('<source>sowing-plan.md</source>');
  expect(sys).toContain('Broad beans go in on 3 March, in bed 4.');
  await shot(main, '03-chat-in-project');
  expect(JSON.stringify(yap.readJson('chats.json'))).not.toContain('Allotment');

  // Another chat moved in from its ⋯, then out again.
  await main.locator('.conv').filter({ hasText: 'Greenhouse heater' }).click({ button: 'right' });
  const menu = main.getByRole('menu').first();
  await menu.getByRole('menuitem', { name: 'Move to project' }).click();
  await main.getByRole('menu').last().getByRole('menuitemradio', { name: 'Allotment 2027' }).click();
  await expect.poll(() => yap.readJson('projects.json').chats['1']).toBe(p.id);
  await expect(main.getByTestId('chat-project-line')).toContainText('Allotment 2027');

  // The project's page lists both chats.
  await main.getByTestId('chat-project-line').getByRole('button').click();
  await expect(page(main).getByRole('list', { name: 'Chats in Allotment 2027' }).getByRole('listitem')).toHaveCount(2);

  // Deleted, asked first: its chats stay, out of it.
  await page(main).getByRole('button', { name: 'More for Allotment 2027' }).click();
  await main.getByRole('menu').first().getByRole('menuitem', { name: 'Delete project…' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toHaveAccessibleName('Delete “Allotment 2027”?');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect(main.getByTestId('chat-projects')).toContainText('No projects yet.');
  expect(yap.readJson('projects.json').projects).toEqual([]);
  expect((await yap.invoke('chats_list')).map((c) => c.title)).toEqual(expect.arrayContaining(['Greenhouse heater', 'When do the broad beans go in?']));
});

test("a big project's steps fold into one line, and its page's line says how the files are read", () => {
  const used = 'Used from the project: staff-handbook.md, part 5, “3. Annual leave”';
  expect(stepsSummary([used])).toBe("Used a part of the project's files");
  expect(stepsSummary([used, 'Used from the project: staff-handbook.pdf, pages 23–24, “3. Annual leave”', 'Read from the project: staff-handbook.md, part 7, “4. Sick pay”', "Searched the project's files: “carry over”"])).toBe(
    "Used 2 parts of the project's files, read another part, searched the project's files"
  );
  expect(knowledgeLine({ tokens: 18_400, searched: true, model: 'Fake Chat' }, 1)).toBe('About 18K tokens: more than Fake Chat reads at once, so each question gets the parts of these files that match it.');
  expect(knowledgeLine({ tokens: 900, searched: false }, 2)).toBe('About 900 tokens: every chat here reads them whole.');
  expect(knowledgeLine(null, 2)).toBe('');
  // A document's mark, never the web's ("Read …" is a page's otherwise).
  expect(chipKind('Read from the project: staff-handbook.md, part 7, “4. Sick pay”')).toBe('doc');
  expect(chipKind(used)).toBe('doc');
});

test('a big project: its files searched in parts, the matching part sent, another read with the tool, the parts said under the answer', async ({ yap, main, fakeLlm, shot }) => {
  await openView(main, 'Chat');
  const p = await yap.invoke('project_create', { name: 'People team', description: 'HR questions' });
  const out = await yap.invoke('project_files_add', { id: p.id, paths: [HANDBOOK] });
  expect(out.added.map((f) => f.name)).toEqual(['staff-handbook.md']);
  expect(out.added[0].tokens).toBeGreaterThan(10_000);

  // The page says the files are read in parts for this model (the fake's 8K).
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByRole('button', { name: /People team/ }).click();
  await expect(page(main).getByTestId('project-knowledge')).toContainText('so each question gets the parts of these files that match it');
  await shot(main, '04-big-project-page');

  // A chat in it: the model reads part 2, then answers.
  await page(main).getByRole('button', { name: 'New chat in People team' }).click();
  fakeLlm.scriptChat([{ tools: [{ name: 'read_project_file', args: { file: 'staff-handbook.md', part: 2 } }] }, { content: 'Full-time staff get 27 days a year (staff-handbook.md, section “3. Annual leave”).' }]);
  const question = 'How many days of annual leave do I get?';
  await box(main).fill(question);
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.cbubble.assistant').last()).toContainText('27 days a year');

  const asked = fakeLlm.requests.filter((r) => Array.isArray(r.messages) && JSON.stringify(r.messages).includes(question) && r.kind === 'other');
  const first = asked[0];
  const sys = systemOf(first);
  // The system prompt: the files listed with their parts, never their text.
  expect(sys).toContain('This chat is in the user\'s project "People team".');
  expect(sys).toContain('too long to send whole');
  expect(sys).toMatch(/<project_files>\n- staff-handbook\.md: \d+ parts: 1 section “1\. Working hours”; 2 /);
  expect(sys).not.toContain('27 days of annual leave');
  // The question: the part that answers it, its source naming the file and its section.
  const q = JSON.stringify(first.messages.filter((m) => m.role === 'user').at(-1));
  expect(q).toContain("From the project's files, the parts that match this question (read_project_file reads any other part)");
  expect(q).toContain('27 days of annual leave');
  expect(q).toMatch(/<source>staff-handbook\.md: part \d+ of \d+, section “3\. Annual leave”<\/source>/);
  // The tools: offered, and the part asked for read.
  expect(first.tools).toEqual(expect.arrayContaining(['read_project_file', 'search_project_files']));
  const results = asked.at(-1).messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(results[0].data.part).toBe(2);
  expect(results[0].data.source).toMatch(/^staff-handbook\.md: part 2 of \d+, section “/);

  // Under the answer: which parts were used, and the one it read.
  const steps = main.locator('.msgs').getByTestId('chat-steps').last();
  await expect(steps).toContainText("Used a part of the project's files, read another part");
  await steps.locator('summary').click();
  await expect(steps.getByRole('list', { name: 'Steps' })).toContainText(/Used from the project: staff-handbook\.md, part \d+, “3\. Annual leave”/);
  await expect(steps.getByRole('list', { name: 'Steps' })).toContainText('Read from the project: staff-handbook.md, part 2, “');
  await shot(main, '05-big-project-answer');
  await theme(yap, main, 'dark');
  await shot(main, '06-big-project-answer-dark');
  await theme(yap, main, 'light');
  const saved = (await yap.invoke('chats_list')).find((c) => c.title.startsWith('How many days'));
  expect(saved).toBeTruthy();
  await yap.invoke('project_delete', { id: p.id });
});
