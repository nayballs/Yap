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
//
// The model is the scripted fake (support/fake-llm.js). Every detail is fictional.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'chat-projects-spec');
const SOWING = path.join(HERE, 'sowing-plan.md');
const SHEET = path.join(HERE, 'seed-costs.xlsx');

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

// Asked out loud (the Voice Assistant, 16 Nov 2026): with a project's page in view, or the new chat its New chat
// started, the question starts its chat in that project, as the first typed question there does.
test("asked out loud on a project's page, or in its new chat, the chat starts in the project", async ({ yap, main, fakeLlm, shot }) => {
  const askAloud = (text) => yap.invoke('e2e_assistant_ask', { text, listenMs: 0 });
  const p = await yap.invoke('project_create', { name: 'Orchard 2027', description: 'Planting the fruit trees' });
  await yap.invoke('project_update', { id: p.id, instructions: 'Name trees by their row and number.' });
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'Projects', exact: true }).click();
  await main.getByTestId('chat-projects').getByRole('button', { name: /Orchard 2027/ }).click();
  await expect(page(main).getByRole('heading', { name: 'Orchard 2027' })).toBeVisible();

  // On the project's page: a new chat in it, the page giving way to the chat.
  fakeLlm.scriptChat([{ content: 'The plum in row 2, number 3.' }]);
  await askAloud('Which tree goes in first?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('row 2, number 3', { timeout: 20_000 });
  await expect(page(main)).toHaveCount(0);
  await expect(main.getByTestId('chat-project-line')).toContainText('In Orchard 2027');
  const sys = systemOf(askedWith(fakeLlm, 'Which tree goes in first?'));
  expect(sys).toContain('This chat is in the user\'s project "Orchard 2027". It\'s for: Planting the fruit trees');
  expect(sys).toContain('<project_instructions>\nName trees by their row and number.\n</project_instructions>');
  const first = (await yap.invoke('chats_list')).find((c) => c.title === 'Which tree goes in first?');
  await expect.poll(() => yap.readJson('projects.json').chats?.[String(first?.id)]).toBe(p.id);
  await shot(main, '04-asked-out-loud-on-the-project-page');

  // Its New chat ("New chat in Orchard 2027", nothing asked yet): the spoken question's chat is in it too.
  await main.getByTestId('chat-project-line').getByRole('button').click();
  await page(main).getByRole('button', { name: 'New chat in Orchard 2027' }).click();
  await expect(main.getByTestId('chat-project-line')).toContainText('New chat in Orchard 2027');
  fakeLlm.scriptChat([{ content: 'Water them weekly in the first summer.' }]);
  await askAloud('How often do new trees need water?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('weekly', { timeout: 20_000 });
  await expect(main.getByTestId('chat-project-line')).toContainText('In Orchard 2027');
  expect(systemOf(askedWith(fakeLlm, 'How often do new trees need water?'))).toContain('<project_instructions>');
  const second = (await yap.invoke('chats_list')).find((c) => c.title === 'How often do new trees need water?');
  await expect.poll(() => yap.readJson('projects.json').chats?.[String(second?.id)]).toBe(p.id);

  // A plain new chat (New chat in the list): in no project, as before.
  await main.getByRole('button', { name: 'New chat', exact: true }).click();
  fakeLlm.scriptChat([{ content: 'About 30 metres.' }]);
  await askAloud('How tall is a fictional lighthouse?');
  await expect(main.locator('.cbubble.assistant').last()).toContainText('30 metres', { timeout: 20_000 });
  await expect(main.getByTestId('chat-project-line')).toHaveCount(0);
  expect(systemOf(askedWith(fakeLlm, 'How tall is a fictional lighthouse?'))).not.toContain('Orchard 2027');
});
