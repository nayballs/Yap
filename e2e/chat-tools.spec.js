// The AI Chat changing part of a note through its tools
// (src-tauri/src/tools.rs + note_text.rs). The model is a local fake
// (support/fake-llm.js) scripted call by call, so this checks Yap's side of
// the tool protocol, not a model's judgement: it searches the note, tries an
// edit whose text is in two places (refused, and told which lines), edits
// the one place, and answers. The note changes there and nowhere else, the
// meeting notepad showing it follows at once, the thread's chips say what
// happened, and they're still there when the chat is opened again.
import { test as base, expect, openView, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

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
        name: 'chat-tools',
        config: {
          llmScopes: {
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const PLAN = 'Ship the beta on Friday.\nAsk Dave about the venue.\nShip the beta docs too.';
const QUESTION = 'Move the beta to Monday in my launch plan';
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });

test('the chat edits one place in a note, and is told when its text is in two', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  const note = await yap.invoke('note_create', { title: 'Launch plan', content: PLAN });
  // The notepad on the note, to see the edit arrive in another window.
  await yap.invoke('notepad_open', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const thoughts = yap.notepad.getByRole('textbox', { name: 'My thoughts' });
  await expect(thoughts).toHaveValue(PLAN);

  fakeLlm.scriptChat([
    { tools: [{ name: 'search_in_note', args: { note_id: note.id, query: 'beta' } }] },
    {
      tools: [
        {
          name: 'edit_note',
          args: { note_id: note.id, field: 'content', old_text: 'Ship the beta', new_text: 'Ship the release' },
        },
      ],
    },
    {
      tools: [
        {
          name: 'edit_note',
          args: {
            note_id: note.id,
            field: 'content',
            old_text: 'Ship the beta on Friday.',
            new_text: 'Ship the beta on Monday.',
          },
        },
      ],
    },
    { content: 'Done: the beta now ships on **Monday**.' },
  ]);

  await openView(main, 'Chat');
  await main.getByRole('textbox', { name: 'Message' }).fill(QUESTION);
  await main.getByRole('button', { name: 'Send' }).click();
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('Done: the beta now ships on Monday.');

  // What happened, in order: the search, the refused edit, the edit.
  const chips = thread.locator('.toolchip');
  const said = [
    'Found "beta" on 2 lines in "Launch plan"',
    'Couldn\'t edit "Launch plan": the text is in 2 places',
    'Edited "Launch plan": "Friday." → "Monday."',
  ];
  await expect(chips).toHaveText(said);
  await shot(main, '01-chat-edited-one-place');

  // The note changed in that one place, and the notepad followed.
  const edited = PLAN.replace('Friday', 'Monday');
  await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === note.id)?.content === edited);
  await expect(thoughts).toHaveValue(edited);
  await shot(yap.notepad, '02-notepad-followed-the-edit');

  // What the model was told: the matches with their lines, why the first
  // edit changed nothing, and the edited lines afterwards.
  const last = fakeLlm.requests.filter((r) => r.tools.includes('edit_note')).at(-1);
  const results = last.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(results).toHaveLength(3);
  expect(results[0].data.hits.map((h) => [h.field, h.line, h.text])).toEqual([
    ['content', 1, 'Ship the beta on Friday.'],
    ['content', 3, 'Ship the beta docs too.'],
  ]);
  expect(results[1].success).toBe(false);
  expect(results[1].error).toContain('appears 2 times in the notes (lines 1, 3)');
  expect(results[2].data).toMatchObject({ changed: true, line: 1 });
  // The instructions steer it to search, then edit.
  expect(last.system).toContain('then call edit_note with old_text copied exactly');

  // The chips stay with the answer: open the conversation again.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(thread).toHaveCount(0);
  await main.locator('.conv').filter({ hasText: QUESTION }).click();
  await expect(thread.locator('.toolchip')).toHaveText(said);

  // The Notes view shows the edited note.
  await openView(main, 'Notes');
  await main.locator('.item').filter({ hasText: 'Launch plan' }).click();
  await expect(main.locator('textarea.raw')).toHaveValue(edited);

  // Tidy up: the notepad isn't reset between tests.
  await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(false);
});

// Yap's folder rule (tools.rs `note_folder`): the chat never makes up a
// folder. A name that isn't a folder is refused, with the folders there are
// and "leave folder empty"; without one the note goes to Personal; a new
// folder needs `new_folder: true` (the user asked for it).
test('the chat never invents a folder: an unknown one is refused, none means Personal', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  const ASK = 'Save my tent packing list as a note';
  fakeLlm.scriptChat([
    { tools: [{ name: 'create_note', args: { title: 'Tent list', content: 'Pegs, poles, mallet', folder: 'Camping' } }] },
    { tools: [{ name: 'create_note', args: { title: 'Tent list', content: 'Pegs, poles, mallet' } }] },
    { content: 'Saved it to **Personal**.' },
  ]);
  await openView(main, 'Chat');
  await main.getByRole('textbox', { name: 'Message' }).fill(ASK);
  await main.getByRole('button', { name: 'Send' }).click();
  const thread = main.locator('.msgs');
  await expect(thread).toContainText('Saved it to Personal.');
  await expect(thread.locator('.toolchip')).toHaveText([
    'Couldn\'t create "Tent list": no folder called "Camping"',
    'Created note: "Tent list"',
  ]);
  await shot(main, '03-chat-folder-refused');

  // One note, in Personal; no Camping folder.
  await expectStore(yap, 'notes.json', (s) =>
    s.notes.filter((n) => n.title === 'Tent list').map((n) => n.folder).join() === 'Personal'
  );
  expect(yap.readJson('notes.json').folders).not.toContain('Camping');

  // What the model was told, and the instructions it had.
  const last = fakeLlm.requests.filter((r) => r.tools.includes('create_note')).at(-1);
  const results = last.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(results[0].success).toBe(false);
  expect(results[0].error).toContain('There\'s no folder called "Camping"');
  expect(results[0].error).toContain('leave folder empty');
  expect(results[0].data.folders).toEqual(expect.arrayContaining(['Personal', 'Meetings']));
  expect(results[1].data.folder).toBe('Personal');
  expect(last.system).toContain('Never make up a folder');
});
