// Chat's document as a page you can edit (2026-10-31; src/lib/views/chat/
// DocPanel.svelte, docEditor.js, docMarkdown.js, DocToolbar.svelte):
// - once Chat has written it, Preview is an editor (TipTap on ProseMirror)
//   with a toolbar over it — text style, code, table, checklist, + insert;
// - the note stays markdown: a word typed changes only its own line, a
//   table cell only its row, and what the toolbar adds is plain markdown;
//   typing is saved as Source's is (a typed version, merged with Chat's
//   writing);
// - while Chat writes, the page is drawn as before and the toolbar waits;
//   Chat's next change lands in the editor with the person's words kept.
// The model is a local fake (support/fake-llm.js) scripted call by call.
import { test as base, expect, openView, expectStore, setConfig } from './support/fixtures.js';
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
        name: 'chat-documents-editor',
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const panel = (main) => main.getByTestId('chat-doc');
const page = (main) => panel(main).locator('.docedit');
const tools = (main) => main.getByTestId('chat-doc-toolbar');
const noteNamed = (s, title) => s.notes.find((n) => n.title === title);

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

function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}

async function ask(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

const LEVELS = '| Level | Pick |\n|---|:---:|\n| 1 | Quick step |\n| 2 | Hide in the reeds |';

/** A build of two sections, written (Gear: a list; Levels: a table), and its note's text. */
async function makeBuild(yap, main, fakeLlm, title) {
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: { title, summary: 'A quiet, fast build for a fictional ranger.', sections: [{ key: 'Gear', intent: 'what to wear' }, { key: 'Levels', intent: 'level by level' }] },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Gear', markdown: '* Two rings of the fox\n* A short bow' }) }] },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Levels', markdown: LEVELS }) }] },
    { content: "It's open on the right." },
  ]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await ask(main, `Make me a ${title.toLowerCase()} for a fictional ranger`);
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  // Once nothing's flowing in: the page you can edit.
  await expect(page(main)).toBeVisible();
  await expect(page(main)).toHaveAttribute('contenteditable', 'true');
  await expect(page(main).locator('table')).toBeVisible();
  const note = (await yap.invoke('notes_list')).find((n) => n.title === title);
  return { id: note.id, content: (await yap.invoke('note_get', { id: note.id })).content };
}

/**
 * The caret to the end of its line (End), and a moment for the editor to
 * take it in: a key the editor acts on itself (Enter) goes where its own
 * selection is, which follows the page's a beat later.
 */
async function end(main) {
  await main.keyboard.press('End');
  await main.waitForTimeout(80);
}

/** The note's text, once it holds `want` (saved after typing). */
async function saved(yap, title, want) {
  let now = null;
  try {
    await expectStore(yap, 'notes.json', (s) => (now = noteNamed(s, title)?.content) === want, 'the note to be saved as typed');
  } catch (e) {
    expect(now, 'the note as saved').toBe(want);
    throw e;
  }
}

test('typed on the page, only the words typed change in the markdown: a paragraph, a table cell', async ({ yap, main, fakeLlm, shot }) => {
  const { id, content } = await makeBuild(yap, main, fakeLlm, 'Ranger build');
  expect(content).toBe(`# Ranger build\n\nA quiet, fast build for a fictional ranger.\n\n## Gear\n\n* Two rings of the fox\n* A short bow\n\n## Levels\n\n${LEVELS}\n`);
  await expect(tools(main).getByRole('button', { name: 'Code' })).toBeEnabled();
  await shot(main, '01-a-page-you-can-edit');

  // A sentence added to the summary: that line, and nothing else.
  await page(main).locator('p').first().click();
  await end(main);
  await main.keyboard.type(' Travel light.');
  const one = content.replace('fictional ranger.', 'fictional ranger. Travel light.');
  await saved(yap, 'Ranger build', one);
  // (The list's `*` markers, the table's centred rule: as Chat wrote them.)

  // A table cell: its row, and only it.
  await page(main).locator('td', { hasText: 'Quick step' }).click();
  await end(main);
  await main.keyboard.type(' first');
  const two = one.replace('| 1 | Quick step |', '| 1 | Quick step first |');
  await saved(yap, 'Ranger build', two);
  // A list item.
  await page(main).locator('li', { hasText: 'A short bow' }).click();
  await end(main);
  await main.keyboard.type(', oiled');
  const three = two.replace('* A short bow', '* A short bow, oiled');
  await saved(yap, 'Ranger build', three);

  // Typing is the person's: a typed version, so no Undo of Chat's.
  const versions = (await yap.invoke('note_versions', { id })).filter((v) => v.field === 'notes');
  expect(versions[0].kind).toBe('typed');
  await expect(panel(main).getByRole('button', { name: "Undo Chat's last change" })).toHaveCount(0);
  // Source says the same, and Preview again is the editor on it.
  await panel(main).getByRole('button', { name: 'Source' }).click();
  await expect(panel(main).getByRole('textbox', { name: 'Document source (Markdown)' })).toHaveValue(three);
  await panel(main).getByRole('button', { name: 'Preview' }).click();
  await expect(page(main)).toContainText('Travel light.');
  // The editor's own undo (Ctrl+Z) takes the typing back.
  await page(main).locator('li', { hasText: 'oiled' }).click();
  await end(main);
  await main.keyboard.type(' x');
  await saved(yap, 'Ranger build', three.replace(', oiled', ', oiled x'));
  await main.keyboard.press('Control+z');
  await saved(yap, 'Ranger build', three);
});

test('the toolbar: text style, code, a checklist, the insert menu and a table, each plain markdown', async ({ yap, main, fakeLlm, shot }) => {
  const { content } = await makeBuild(yap, main, fakeLlm, 'Heron build');
  const after = (added) => content.replace('fictional ranger.\n', `fictional ranger.\n\n${added}\n`);
  // A new paragraph under the summary, made a heading.
  await page(main).locator('p').first().click();
  await end(main);
  await main.keyboard.press('Enter');
  await main.keyboard.type('Notes');
  await tools(main).getByRole('button', { name: /^Text style/ }).click();
  await expect(main.getByRole('menuitemradio', { name: 'Paragraph' })).toHaveAttribute('aria-checked', 'true');
  await shot(main, '02-text-style');
  await main.getByRole('menuitemradio', { name: 'Heading 2' }).click();
  await expect(page(main).locator('h3', { hasText: 'Notes' })).toBeVisible();
  await expect(tools(main).getByRole('button', { name: 'Text style: Heading 2' })).toBeVisible();
  await saved(yap, 'Heron build', after('## Notes'));

  // Code: a word selected, then the button (pressed while the caret's in it).
  await main.keyboard.press('Enter');
  await main.keyboard.type('Use quick step');
  for (let i = 0; i < 'quick step'.length; i++) await main.keyboard.press('Shift+ArrowLeft');
  await tools(main).getByRole('button', { name: 'Code' }).click();
  await expect(tools(main).getByRole('button', { name: 'Code' })).toHaveAttribute('aria-pressed', 'true');
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`'));

  // A checklist from a new line (code off first: it goes on past the end
  // of the word, as typing on would); its box ticked on the page.
  await end(main);
  await tools(main).getByRole('button', { name: 'Code' }).click();
  await expect(tools(main).getByRole('button', { name: 'Code' })).toHaveAttribute('aria-pressed', 'false');
  await main.keyboard.press('Enter');
  await main.keyboard.type('Pack rope');
  await tools(main).getByRole('button', { name: 'Checklist' }).click();
  await expect(tools(main).getByRole('button', { name: 'Checklist' })).toHaveAttribute('aria-pressed', 'true');
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [ ] Pack rope'));
  const tick = page(main).locator('li.task', { hasText: 'Pack rope' }).locator('input.taskbox');
  await tick.click();
  await expect(tick).toBeChecked();
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope'));
  // Ctrl+Enter in the item ticks it too (and back).
  await page(main).locator('li.task', { hasText: 'Pack rope' }).locator('p').click();
  await main.keyboard.press('Control+Enter');
  await expect(tick).not.toBeChecked();
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [ ] Pack rope'));
  await main.keyboard.press('Control+Enter');
  await expect(tick).toBeChecked();
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope'));
  await shot(main, '03-checklist');

  // Out of the list (Enter on an empty item), then + → Quote.
  await page(main).locator('li.task', { hasText: 'Pack rope' }).click();
  await end(main);
  await main.keyboard.press('Enter');
  await main.keyboard.press('Enter');
  await main.keyboard.type('Said once');
  await tools(main).getByRole('button', { name: 'Insert', exact: true }).click();
  await expect(main.getByRole('menu', { name: 'Insert' }).getByRole('menuitem')).toHaveText(['Divider', 'Quote', 'Bulleted list', 'Numbered list', 'Code block']);
  await shot(main, '04-insert-menu');
  await main.getByRole('menuitem', { name: 'Quote' }).click();
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope\n\n> Said once'));

  // Out of the quote; a table on the new line, its first cell typed into
  // (the rest of the note as it was).
  await main.keyboard.press('Enter');
  await main.keyboard.press('Enter');
  await tools(main).getByRole('button', { name: 'Insert a table' }).click();
  await expect(page(main).locator('table')).toHaveCount(2);
  await main.keyboard.type('Item');
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope\n\n> Said once\n\n| Item |  |  |\n|---|---|---|\n|  |  |  |\n|  |  |  |'));
  // In a table, the table button offers its rows and columns.
  await tools(main).getByRole('button', { name: 'Table', exact: true }).click();
  await expect(main.getByRole('menu', { name: 'Table' }).getByRole('menuitem')).toHaveText([
    'Add a row below',
    'Add a column to the right',
    'Delete this row',
    'Delete this column',
    'Delete the table',
  ]);
  await main.getByRole('menuitem', { name: 'Add a row below' }).click();
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope\n\n> Said once\n\n| Item |  |  |\n|---|---|---|\n|  |  |  |\n|  |  |  |\n|  |  |  |'));
  await tools(main).getByRole('button', { name: 'Table', exact: true }).click();
  await main.getByRole('menuitem', { name: 'Delete the table' }).click();
  await expect(page(main).locator('table')).toHaveCount(1);
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope\n\n> Said once'));

  // + → Divider, then a code block on the line after it.
  await page(main).locator('blockquote').click();
  await end(main);
  await main.keyboard.press('Enter');
  await main.keyboard.press('Enter');
  await tools(main).getByRole('button', { name: 'Insert', exact: true }).click();
  await main.getByRole('menuitem', { name: 'Divider' }).click();
  await main.keyboard.type('let rope = 30;');
  await tools(main).getByRole('button', { name: 'Insert', exact: true }).click();
  await main.getByRole('menuitem', { name: 'Code block' }).click();
  await saved(yap, 'Heron build', after('## Notes\n\nUse `quick step`\n\n- [x] Pack rope\n\n> Said once\n\n---\n\n```\nlet rope = 30;\n```'));
  await shot(main, '05-after-the-toolbar');

  // Dark, as it looks there.
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '06-dark');
  await setConfig(yap, main, { theme: 'light' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('light');
});

test("while Chat writes the toolbar waits; its next change lands in the editor with the person's words kept", async ({ yap, main, fakeLlm }) => {
  const { id, content } = await makeBuild(yap, main, fakeLlm, 'Otter build');
  // The person adds a line to the summary.
  await page(main).locator('p').first().click();
  await end(main);
  await main.keyboard.type(' Mine.');
  const mine = content.replace('fictional ranger.', 'fictional ranger. Mine.');
  await saved(yap, 'Otter build', mine);

  // Chat rewrites Gear (its call streamed slowly): while it writes, the
  // page is drawn and the toolbar waits.
  fakeLlm.scriptChat([
    { tools: [{ name: 'rewrite_section', args: { note_id: id, heading: 'Gear', markdown: '- A cloak of reeds' } }], stream: { gapMs: 900 } },
    { content: 'Swapped the gear.' },
  ]);
  await ask(main, 'Change the gear to a cloak of reeds');
  await expect(tools(main).getByRole('button', { name: 'Code' })).toBeDisabled();
  await expect(tools(main)).toContainText('Chat is writing…');
  await expect(page(main)).toHaveCount(0);
  await expect(thread(main)).toContainText('Swapped the gear.', { timeout: 20_000 });
  // The editor again: Chat's change and the person's words, both.
  await expect(page(main)).toBeVisible();
  await expect(page(main)).toContainText('A cloak of reeds');
  await expect(page(main)).toContainText('Mine.');
  await expect(tools(main).getByRole('button', { name: 'Code' })).toBeEnabled();
  await saved(yap, 'Otter build', mine.replace('* Two rings of the fox\n* A short bow', '- A cloak of reeds'));

  // Undo (Chat's last change): back in the editor, the change marked; the person's words stay.
  await panel(main).getByRole('button', { name: "Undo Chat's last change" }).click();
  await expect(page(main)).toContainText('A short bow');
  await expect(page(main).locator('[data-changed]').first()).toBeVisible();
  await saved(yap, 'Otter build', mine);

  // Find in the editor (Ctrl+F) lights its words.
  await page(main).locator('p').first().click();
  await main.keyboard.press('Control+f');
  const find = panel(main).getByRole('searchbox', { name: 'Find in the document' }).or(panel(main).getByRole('textbox', { name: 'Find in the document' }));
  await find.fill('reeds');
  await expect(panel(main).locator('.find .count')).toHaveText('1 of 1');
  await find.press('Escape');
});
