// Documents beside the chat, the rich tools and the panel's two views
// (src-tauri/src/chat/documents/rich.rs, blocks.rs; src/lib/views/chat/
// DocPanel.svelte, chatDoc.svelte.js):
// - A model that handles tools well (here a fake named like the GPT-5
//   family: `documents::rich::handles_rich`) gets read_document /
//   edit_document / document_history / restore_document in place of
//   rewrite_section, and the rule that says how to use them.
// - A table row added by `edit_document` (a replace of the whole table):
//   the page shows it, the changed block marked a moment, one Chat version.
// - A stale edit (the person changed the block in Source while the model
//   held its read) is refused with the block as it stands now; their words
//   stay.
// - "Change this…" on a selected passage: the question carries it (quoted
//   over the box and above the question), and the model is told its block —
//   typed, dictated into the box, or asked out loud with Chat in view (from
//   another app the quote stays for the box).
// - restore_document brings an earlier version back, marked Chat's: the
//   panel's Undo takes it back.
// - Preview / Source (the markdown coloured and editable), find (Ctrl+F),
//   Copy as Markdown; light and dark screenshots.
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
        name: 'chat-documents-rich',
        config: {
          // Named like a strong model: the rich set (documents::rich::handles_rich).
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5' } },
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
const docBody = (main) => panel(main).locator('.docbody');
const chips = (main) => thread(main).locator('.toolchip');

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

/** The newest tool result of this turn with `key` in its data (a read, a history). */
function lastResult(record, key) {
  for (const m of [...record.messages].reverse()) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (r?.data?.[key] != null) return r;
    } catch {
      /* not JSON */
    }
  }
  return null;
}

/** A read's first block of `type`. */
const blockOf = (record, type) => lastResult(record, 'blocks').data.blocks.find((b) => b.type === type);

function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function ask(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** A build of two sections, written: Gear (a list) and Levels (a table). */
async function makeBuild(main, fakeLlm, title) {
  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'create_document',
          args: { title, summary: 'A quiet, fast build for a fictional ranger.', sections: [{ key: 'Gear', intent: 'what to wear' }, { key: 'Levels', intent: 'level by level' }] },
        },
      ],
    },
    { tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Gear', markdown: '- Two rings of the fox\n- A short bow' }) }] },
    {
      tools: [{ name: 'write_section', args: (r) => ({ note_id: madeDoc(r), key: 'Levels', markdown: '| Level | Pick |\n|---|---|\n| 1 | Quick step |\n| 2 | Hide in the reeds |' }) }],
    },
    { content: "It's open on the right." },
  ]);
  await freshChat(main);
  await ask(main, `Make me a ${title.toLowerCase()} for a fictional ranger`);
  await expect(thread(main)).toContainText("It's open on the right.", { timeout: 20_000 });
  await expect(docBody(main).locator('table')).toBeVisible();
}

const noteNamed = (s, title) => s.notes.find((n) => n.title === title);

test('a strong model gets the rich tools, and adds a table row with edit_document', async ({ yap, main, fakeLlm, shot }) => {
  await makeBuild(main, fakeLlm, 'Ranger build');
  // The rich set: read and edit by block, no rewrite_section; its rule.
  const first = fakeLlm.requests.find((r) => r.user.includes('ranger build'));
  for (const name of ['create_document', 'write_section', 'read_document', 'edit_document', 'document_history', 'restore_document']) {
    expect(first.tools).toContain(name);
  }
  expect(first.tools).not.toContain('rewrite_section');
  expect(first.system).toContain('read_document, then edit_document with ops on its block ids');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Ranger build');
  const before = (await yap.invoke('note_versions', { id: doc.id })).filter((v) => v.field === 'notes').length;

  // "Add a row": read, then a replace of the whole table.
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_document', args: { note_id: doc.id } }] },
    {
      tools: [
        {
          name: 'edit_document',
          args: (r) => {
            const t = blockOf(r, 'table');
            return { note_id: doc.id, ops: [{ op: 'replace', id: t.id, if_hash: t.hash, markdown: `${t.text}\n| 3 | Shadow step |` }] };
          },
        },
      ],
    },
    { content: 'Added level 3.' },
  ]);
  await ask(main, 'Add a row for level 3 to the table');
  await expect(docBody(main).locator('table')).toContainText('Shadow step', { timeout: 20_000 });
  // The changed block marked a moment as it lands.
  await expect(docBody(main).locator('.docmd > [data-changed]')).toHaveCount(1);
  await expect(thread(main)).toContainText('Added level 3.');
  await expect(chips(main).filter({ hasText: 'Ranger build' }).last()).toHaveText('Changed a table in "Ranger build"');
  await shot(main, '01-a-table-row-added');
  // The model was told the document's rev, and that this changes it.
  const step = fakeLlm.requests.find((r) => r.user.includes('Add a row for level 3'));
  expect(step.user).toMatch(/"Ranger build" \(note_id \d+, rev \d+\)/);
  expect(step.user).toContain('read_document, then edit_document');
  // The read gave blocks with ids and hashes.
  const read = fakeLlm.requests.filter((r) => r.user.includes('Add a row for level 3')).at(-1);
  const blocks = lastResult(read, 'blocks').data.blocks;
  expect(blocks.map((b) => b.type)).toEqual(['heading', 'paragraph', 'heading', 'list', 'heading', 'table']);
  expect(blocks.every((b) => /^[a-z][0-9a-f]{5}(-\d+)?$/.test(b.id) && /^[0-9a-f]{8}$/.test(b.hash))).toBe(true);
  // One version, Chat's.
  const after = (await yap.invoke('note_versions', { id: doc.id })).filter((v) => v.field === 'notes');
  expect(after.length).toBe(before + 1);
  expect(after[0].kind).toBe('chat');
  await expectStore(yap, 'notes.json', (s) => noteNamed(s, 'Ranger build')?.content.includes('| 2 | Hide in the reeds |\n| 3 | Shadow step |'));
});

test("a stale edit is refused with the block as it stands now: the person's words stay", async ({ yap, main, fakeLlm }) => {
  await makeBuild(main, fakeLlm, 'Swamp build');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Swamp build');
  const held = gate();
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_document', args: { note_id: doc.id } }] },
    {
      // Held while the person edits the list the model just read.
      gate: held.promise,
      tools: [
        {
          name: 'edit_document',
          args: (r) => {
            const l = blockOf(r, 'list');
            return { note_id: doc.id, ops: [{ op: 'replace', id: l.id, if_hash: l.hash, markdown: '- Two rings of the fox\n- A long bow' }] };
          },
        },
      ],
    },
    { content: 'You changed the gear yourself, so I left it.' },
  ]);
  await ask(main, 'Swap the short bow for a long bow');
  await expect.poll(() => fakeLlm.requests.some((r) => r.gated)).toBe(true);

  // The person changes that list in Source meanwhile; it's saved.
  await panel(main).getByRole('button', { name: 'Source' }).click();
  const area = panel(main).getByRole('textbox', { name: 'Document source (Markdown)' });
  await expect(area).toBeFocused();
  const now = await area.inputValue();
  await area.fill(now.replace('- A short bow', '- A short bow, oiled'));
  await expectStore(yap, 'notes.json', (s) => noteNamed(s, 'Swamp build')?.content.includes('- A short bow, oiled'));
  held.open();

  await expect(thread(main)).toContainText('You changed the gear yourself', { timeout: 20_000 });
  await expect(chips(main).filter({ hasText: 'Swamp build' }).last()).toHaveText('Didn\'t change "Swamp build": the user changed that part');
  // Refused, with the block as it is now; all or nothing: their words stay.
  const last = fakeLlm.requests.filter((r) => r.user.includes('long bow')).at(-1);
  const refused = last.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content)).at(-1);
  expect(refused.success).toBe(false);
  expect(refused.error).toContain('the user changed block');
  expect(refused.data.now[0].text).toBe('- Two rings of the fox\n- A short bow, oiled');
  await expectStore(yap, 'notes.json', (s) => {
    const c = noteNamed(s, 'Swamp build')?.content ?? '';
    return c.includes('- A short bow, oiled') && !c.includes('A long bow');
  });
});

test('"Change this…" on a selected passage: quoted, sent with the question, and the model is told its block', async ({ yap, main, fakeLlm, shot }) => {
  await makeBuild(main, fakeLlm, 'Fox build');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Fox build');
  // Select the summary paragraph on the page with the mouse (a triple-click:
  // the selection ends at the next block's start, which isn't part of it).
  const para = docBody(main).locator('.docmd > p').first();
  await para.click({ clickCount: 3 });
  const pop = main.getByTestId('chat-doc-selection');
  await expect(pop).toBeVisible();
  await expect(pop.getByRole('button')).toHaveText(['Ask about this', 'Change this…']);
  await shot(main, '02-a-selection-popover');
  await pop.getByRole('button', { name: 'Change this…' }).click();
  // Quoted over the box, which asks how.
  const quote = main.getByTestId('chat-quote');
  await expect(quote).toContainText('A quiet, fast build for a fictional ranger.');
  await expect(quote).toContainText('Change in Fox build:');
  await expect(box(main)).toBeFocused();
  await expect(box(main)).toHaveAttribute('placeholder', 'How should it change?');
  await shot(main, '03-quoted-over-the-box');

  fakeLlm.scriptChat([
    {
      tools: [
        {
          name: 'edit_document',
          args: (r) => {
            const m = /blocks? (p[0-9a-f]{5}(?:-\d+)?) \(hash ([0-9a-f]{8})\)/.exec(r.user);
            return { note_id: doc.id, ops: [{ op: 'replace', id: m[1], if_hash: m[2], markdown: 'A quiet build for a fictional ranger.' }] };
          },
        },
      ],
    },
    { content: 'Shorter now.' },
  ]);
  await box(main).fill('Make it shorter');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(thread(main)).toContainText('Shorter now.', { timeout: 20_000 });
  await expect(quote).toHaveCount(0);
  // The question keeps its quote; the model was told the passage and its block.
  await expect(thread(main).getByTestId('chat-quote-sent')).toHaveText('A quiet, fast build for a fictional ranger.');
  const step = fakeLlm.requests.find((r) => r.user.includes('Make it shorter'));
  expect(step.user).toContain('The user selected this passage in "Fox build"');
  expect(step.user).toMatch(/It's in block p[0-9a-f]{5} \(hash [0-9a-f]{8}\) of the document/);
  await expect(docBody(main).locator('.docmd > p').first()).toHaveText('A quiet build for a fictional ranger.');
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.text === 'Make it shorter' && m.quote?.noteId === doc.id && m.quote.text.startsWith('A quiet, fast')))
  );
  await shot(main, '04-the-passage-changed');
});

// However the question comes it takes the quote, as a typed one does:
// dictated into the box (test mode's stand-in for the mic; "press enter"
// said, so it's sent by itself), or asked out loud with Chat in view (the
// Voice Assistant). The maintainer's try, 31 Oct 2026: a table row
// selected, "Change this…", then the question said out loud — it went
// without the quote, and the quote stayed over the box after the answer.

/** "Change this…" on the table's level-2 row (a triple-click in its cell). */
async function changeRow(main) {
  await docBody(main).locator('td', { hasText: 'Hide in the reeds' }).click({ clickCount: 3 });
  const pop = main.getByTestId('chat-doc-selection');
  await expect(pop).toBeVisible();
  await pop.getByRole('button', { name: 'Change this…' }).click();
  await expect(main.getByTestId('chat-quote')).toContainText('Hide in the reeds');
}

/** The model's edit of the block it was told the selection is in. */
const editSelected = (id, markdown) => ({
  tools: [
    {
      name: 'edit_document',
      args: (r) => {
        const m = /blocks? ([a-z][0-9a-f]{5}(?:-\d+)?) \(hash ([0-9a-f]{8})\)/.exec(r.user);
        return { note_id: id, ops: [{ op: 'replace', id: m[1], if_hash: m[2], markdown }] };
      },
    },
  ],
});

const LEVELS_21 = '| Level | Pick |\n|---|---|\n| 1 | Quick step |\n| 21–22 | Hide in the reeds |';

test('a question dictated into the box takes the quoted passage, and the quote goes', async ({ yap, main, fakeLlm }) => {
  await makeBuild(main, fakeLlm, 'Heron build');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Heron build');
  await changeRow(main);
  fakeLlm.scriptChat([editSelected(doc.id, LEVELS_21), { content: 'Levels 21 to 22 now.' }]);
  await yap.invoke('e2e_chat_dictation', { text: 'Make this level twenty-one to twenty-two only, press enter' });
  await expect(thread(main)).toContainText('Levels 21 to 22 now.', { timeout: 20_000 });
  // The chip went with the question, which keeps it.
  await expect(main.getByTestId('chat-quote')).toHaveCount(0);
  await expect(thread(main).locator('.cbubble.user').last()).toContainText('Make this level twenty-one to twenty-two only');
  await expect(thread(main).getByTestId('chat-quote-sent').last()).toContainText('Hide in the reeds');
  // The model was told the passage and its block (the table): no reading
  // the whole document to find it.
  const step = fakeLlm.requests.findLast((r) => r.user.includes('Make this level twenty-one'));
  expect(step.user).toContain('The user selected this passage in "Heron build"');
  expect(step.user).toMatch(/It's in block t[0-9a-f]{5} \(hash [0-9a-f]{8}\)/);
  await expect(docBody(main).locator('table')).toContainText('21–22');
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.text.startsWith('Make this level twenty-one') && m.quote?.noteId === doc.id && m.quote.text.includes('Hide in the reeds')))
  );
});

test('asked out loud with Chat in view, a question takes the quote too; from another app it stays', async ({ yap, main, fakeLlm }) => {
  await makeBuild(main, fakeLlm, 'Otter build');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Otter build');
  await changeRow(main);
  const quote = main.getByTestId('chat-quote');

  // From another app (Yap's window not in view): the question is about
  // something else, and the quote stays for the box.
  await yap.invoke('e2e_focus', { focused: false });
  try {
    fakeLlm.scriptChat([{ content: 'Sunny all week.' }]);
    await yap.invoke('e2e_assistant_ask', { text: 'What is the weather like', listenMs: 0 });
    await expect(thread(main)).toContainText('Sunny all week.', { timeout: 20_000 });
  } finally {
    await yap.invoke('e2e_focus', { focused: true });
  }
  const away = fakeLlm.requests.find((r) => r.user.includes('What is the weather like'));
  expect(away.user).not.toContain('The user selected this passage');
  await expect(quote).toContainText('Hide in the reeds');

  // With Chat in front: it goes with the spoken question, and the chip goes.
  fakeLlm.scriptChat([editSelected(doc.id, LEVELS_21), { content: 'Levels 21 to 22 now.' }]);
  await yap.invoke('e2e_assistant_ask', { text: 'Make this level twenty-one to twenty-two only.', listenMs: 0 });
  await expect(thread(main)).toContainText('Levels 21 to 22 now.', { timeout: 20_000 });
  await expect(quote).toHaveCount(0);
  await expect(thread(main).getByTestId('chat-quote-sent').last()).toContainText('Hide in the reeds');
  const step = fakeLlm.requests.findLast((r) => r.user.includes('Make this level twenty-one'));
  expect(step.user).toContain('The user selected this passage in "Otter build"');
  expect(step.user).toMatch(/It's in block t[0-9a-f]{5} \(hash [0-9a-f]{8}\)/);
  // (Nor was the window in front looked at: the passage is what it's about.)
  expect(fakeLlm.requests.some((r) => r.kind === 'screenQuestion' && r.user.includes('Make this level twenty-one'))).toBe(false);
  await expect(docBody(main).locator('table')).toContainText('21–22');
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.voice && m.text.startsWith('Make this level twenty-one') && m.quote?.noteId === doc.id))
  );
});

test('restore_document brings an earlier version back, and Undo takes the restore back', async ({ yap, main, fakeLlm }) => {
  await makeBuild(main, fakeLlm, 'Marsh build');
  const doc = (await yap.invoke('notes_list')).find((n) => n.title === 'Marsh build');
  // A change first.
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_document', args: { note_id: doc.id } }] },
    {
      tools: [
        {
          name: 'edit_document',
          args: (r) => {
            const l = blockOf(r, 'list');
            return { note_id: doc.id, ops: [{ op: 'replace', id: l.id, if_hash: l.hash, markdown: '- A cloak of reeds' }] };
          },
        },
      ],
    },
    { content: 'Changed the gear.' },
  ]);
  await ask(main, 'Change the gear to a cloak of reeds');
  await expect(docBody(main)).toContainText('A cloak of reeds', { timeout: 20_000 });
  // Then "put it back as it was": the history, and the version before.
  fakeLlm.scriptChat([
    { tools: [{ name: 'document_history', args: { note_id: doc.id } }] },
    { tools: [{ name: 'restore_document', args: (r) => ({ note_id: doc.id, version: lastResult(r, 'versions').data.versions[1].version }) }] },
    { content: 'Version back.' },
  ]);
  await ask(main, 'Put the gear back the way it was');
  await expect(thread(main)).toContainText('Version back.', { timeout: 20_000 });
  await expect(docBody(main)).toContainText('Two rings of the fox');
  await expect(docBody(main)).not.toContainText('A cloak of reeds');
  await expect(chips(main).filter({ hasText: 'Marsh build' }).last()).toHaveText(/^\s*Restored version \d+ of "Marsh build"$/);
  const history = fakeLlm.requests.filter((r) => r.user.includes('the way it was')).at(-1);
  const listed = lastResult(history, 'versions').data.versions;
  expect(listed[0].by).toBe('Chat');
  expect(listed[0].current).toBe(true);
  const newest = (await yap.invoke('note_versions', { id: doc.id })).filter((v) => v.field === 'notes')[0];
  expect([newest.kind, newest.label]).toEqual(['restored', 'Chat']);
  // Undo: the restore is Chat's change, so it goes back.
  await panel(main).getByRole('button', { name: "Undo Chat's last change" }).click();
  await expect(docBody(main)).toContainText('A cloak of reeds');
});

test('Preview and Source, find and copy, light and dark', async ({ yap, main, fakeLlm, shot }) => {
  await makeBuild(main, fakeLlm, 'Reed build');
  const preview = panel(main).getByRole('button', { name: 'Preview' });
  const source = panel(main).getByRole('button', { name: 'Source' });
  await expect(preview).toHaveAttribute('aria-pressed', 'true');

  // Source: the markdown, coloured (a heading's marks quiet), editable.
  await source.click();
  await expect(source).toHaveAttribute('aria-pressed', 'true');
  const area = panel(main).getByRole('textbox', { name: 'Document source (Markdown)' });
  await expect(area).toHaveValue(/## Gear\n\n- Two rings of the fox/);
  await expect(panel(main).locator('.srcmirror .md-s-heading').first()).toHaveText('# Reed build');
  await expect(panel(main).locator('.srcmirror .md-s-mark').filter({ hasText: '|' }).first()).toBeVisible();
  // The coloured copy says exactly what the box does, character for character.
  const mirror = await panel(main).locator('.srcmirror').evaluate((el) => el.textContent.replace(/\n​$/, ''));
  expect(mirror).toBe(await area.inputValue());
  await shot(main, '05-source');
  // Typed in Source: saved, and on the page.
  await area.press('Control+End');
  await area.pressSequentially('\nPack a spare string.');
  await expectStore(yap, 'notes.json', (s) => noteNamed(s, 'Reed build')?.content.includes('Pack a spare string.'));
  // ←/→ on the switch.
  await source.focus();
  await main.keyboard.press('ArrowLeft');
  await expect(preview).toHaveAttribute('aria-pressed', 'true');
  await expect(docBody(main)).toContainText('Pack a spare string.');

  // Find (Ctrl+F in the panel): every match lit, the one it's on counted.
  await docBody(main).focus();
  await main.keyboard.press('Control+f');
  const find = panel(main).getByRole('searchbox', { name: 'Find in the document' }).or(panel(main).getByRole('textbox', { name: 'Find in the document' }));
  await expect(find).toBeFocused();
  await find.fill('fox');
  const count = panel(main).locator('.find .count');
  await expect(count).toHaveText('1 of 1');
  await find.fill('re');
  await expect(count).toHaveText(/^1 of \d+$/);
  await find.press('Enter');
  await expect(count).toHaveText(/^2 of \d+$/);
  await shot(main, '06-find');
  // Esc closes the bar, not the panel.
  await find.press('Escape');
  await expect(find).toHaveCount(0);
  await expect(panel(main)).toBeVisible();
  // In Source too: the markdown's own words.
  await source.click();
  await area.focus();
  await main.keyboard.press('Control+f');
  await find.fill('| 2 |');
  await expect(count).toHaveText('1 of 1');
  await find.press('Escape');

  // Copy as Markdown (⋯): the whole document, sections still to write left out.
  await main.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  await panel(main).getByRole('button', { name: 'More for the document' }).click();
  await expect(main.getByRole('menuitem', { name: 'Export as Markdown…' })).toBeVisible();
  await main.getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect.poll(() => main.evaluate(() => window.__copied)).toMatch(/^# Reed build\n\nA quiet, fast build for a fictional ranger\.\n\n## Gear\n\n- Two rings of the fox/);

  // The scroll is kept per view: Preview, scrolled, then Source and back.
  await preview.click();
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '07-preview-dark');
  await source.click();
  await shot(main, '08-source-dark');
  await setConfig(yap, main, { theme: 'light' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('light');
  await preview.click();
  await shot(main, '09-preview-light');
});
