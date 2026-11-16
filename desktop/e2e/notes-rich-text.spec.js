// Notes as pages you can format (2026-10-31; desktop/src/lib/components/richtext/:
// RichNote.svelte on the rich text editor Chat's documents edit on, TipTap
// on ProseMirror): the Notes view's My thoughts and the meeting notepad's.
// The quick note's page: quick-note.spec.js; the markdown on its own:
// note-markdown.spec.js.
// - typed, and formatted with the toolbar or markdown as typed ("## ",
//   "[ ] "), a note is saved as exact markdown: a change rewrites only its
//   own lines;
// - a checklist's box ticked on the page: its line, in place;
// - each save a version (notes/versions.rs: "Typed", "Dictated");
// - Preview / Source in the Notes view (Source the markdown in a plain
//   box), remembered; what the editor can't hold kept whole, read-only;
// - Yap's paste of a dictation goes in at the caret as plain text (a
//   spoken list a list), its version "Dictated";
// - the notepad's My thoughts the same page while a test meeting records.
import { test as base, expect, openView, expectStore } from './support/fixtures.js';
import { noteText, expectText, pageIn, selectText } from './support/notePage.js';

const test = base.extend({
  yapOptions: [{ name: 'notes-rich-text', config: { meetingReminder: 'never' } }, { scope: 'worker', option: true }],
});

const raw = (page) => page.getByRole('textbox', { name: 'My thoughts' });
const tools = (page) => page.getByTestId('note-toolbar');
const noteIn = (s, id) => s?.notes?.find((n) => n.id === id);
const saved = (yap, id, want) => expectStore(yap, 'notes.json', (s) => noteIn(s, id)?.content === want, `the note to say ${JSON.stringify(want)}`);
// Newest first.
const versions = (yap, id) => yap.invoke('note_versions', { id }).then((v) => v.filter((x) => x.field === 'notes'));
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);

const folderRow = (main, name) => main.locator('.folderlist').getByRole('button', { name: new RegExp(`^${name}( \\d+)?$`) });
const noteRow = (main, title) => main.locator('.items .item').filter({ hasText: title }).first();

/** Note `title` (in `folder`) open in the Notes view; `page`: wait for its page. */
async function openNote(main, title, { folder = 'Personal', page = true } = {}) {
  await openView(main, 'Notes');
  await folderRow(main, folder).click();
  await noteRow(main, title).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue(title);
  if (page) await pageIn(raw(main));
}

/** Yap's paste of a dictation (its Ctrl+V), as the page gets it: plain text. */
function paste(box, text) {
  return box.evaluate((el, t) => {
    const data = new DataTransfer();
    data.setData('text/plain', t);
    el.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  }, text);
}

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
});

// First: the page's chunk must not have loaded in this window yet (a retry
// starts a fresh test Yap).
test('Notes: typing as the page comes in: the keyboard goes with it, nothing typed is lost', async ({ yap, main }) => {
  // 16 Nov 2026: meeting-crash's note, typed into the plain box as the page
  // came in, was lost now and then. The page took the caret but the keyboard
  // only on the next frame (TipTap's focus waits for one), and a busy PC can
  // draw none for seconds: what was typed meanwhile went to no box. Here the
  // page's chunk is held (CDP's Fetch domain) while the plain box is typed
  // in, frames are held as a busy PC's are, and the next words go in the
  // moment the page is there.
  const cdp = await main.context().newCDPSession(main);
  const held = [];
  cdp.on('Fetch.requestPaused', (e) => held.push(e.requestId));
  await cdp.send('Fetch.enable', { patterns: [{ urlPattern: '*NoteThoughts*', requestStage: 'Request' }] });
  const release = async () => {
    for (const requestId of held.splice(0)) await cdp.send('Fetch.continueRequest', { requestId });
  };
  const frames = (on) =>
    main.evaluate((on) => {
      if (on) {
        if (window.__heldRaf) window.requestAnimationFrame = window.__heldRaf;
        window.__heldRaf = null;
      } else {
        window.__heldRaf = window.requestAnimationFrame;
        window.requestAnimationFrame = () => 0;
      }
    }, on);
  try {
    const n = await yap.invoke('note_create', { title: 'Seed order' });
    await openNote(main, 'Seed order', { page: false });
    // The plain box stands in while the page loads: typed in.
    const box = main.locator('textarea[data-rich-fallback]');
    await expect(box).toBeVisible();
    await expect.poll(() => held.length, { message: "the page's chunk, held" }).toBeGreaterThan(0);
    await box.focus();
    await main.keyboard.insertText('Order beans');
    // No frame drawn for a while; the page comes in, and the next words straight away.
    await frames(false);
    await release();
    await pageIn(raw(main));
    await main.keyboard.insertText(' and peas');
    await frames(true);
    await expectText(raw(main), 'Order beans and peas');
    await saved(yap, n.id, 'Order beans and peas');
  } finally {
    await frames(true).catch(() => {});
    await release().catch(() => {});
    await cdp.send('Fetch.disable').catch(() => {});
    await cdp.detach().catch(() => {});
  }
});

test('Notes: typing that goes on without a pause is saved as it goes, not only once it stops', async ({ yap, main }) => {
  // Saved only after a pause, a note typed without one was on disk only once
  // it stopped: a crash meanwhile lost all of it. Now at most the last two
  // seconds (core/typingSave.js). A key every 200 ms for four seconds.
  const n = await yap.invoke('note_create', { title: 'Allotment diary' });
  await openNote(main, 'Allotment diary');
  await selectText(raw(main), 0);
  const keys = 'abcdefghijklmnopqrst';
  // (A read as Yap replaces the file can fail: not saved yet, as far as this knows.)
  const onDisk = () => {
    try {
      return noteIn(yap.readJson('notes.json'), n.id)?.content || '';
    } catch {
      return '';
    }
  };
  let savedAt = -1;
  for (const [i, k] of [...keys].entries()) {
    await main.keyboard.insertText(k);
    await main.waitForTimeout(200);
    if (savedAt < 0 && onDisk()) savedAt = i;
  }
  expect(savedAt, 'saved while the typing went on').toBeGreaterThanOrEqual(0);
  expect(savedAt).toBeLessThan(keys.length - 3);
  await saved(yap, n.id, keys);
});

test('Notes: typed and formatted on the page, saved as exact markdown; a box ticked; each a typed version', async ({ yap, main, shot }) => {
  const n = await yap.invoke('note_create', { title: 'Garden plan', content: 'Beds by the fence\nBeans in March' });
  await openNote(main, 'Garden plan');
  await expectText(raw(main), 'Beds by the fence\nBeans in March');
  await expect(tools(main).getByRole('button', { name: 'Checklist' })).toBeEnabled();
  await shot(main, '01-my-thoughts-as-a-page');

  // Words at the end of the first line: that line, and only it.
  await selectText(raw(main), 'Beds by the fence'.length);
  await main.keyboard.type(' and the shed');
  let md = 'Beds by the fence and the shed\nBeans in March';
  await saved(yap, n.id, md);

  // At the end: a paragraph, "## " a heading, "[ ] " a checklist item.
  await selectText(raw(main), md.length);
  await main.keyboard.press('Enter');
  await main.keyboard.type('## Tasks');
  await main.keyboard.press('Enter');
  await main.keyboard.type('[ ] Order seeds');
  await expect(raw(main).locator('h3', { hasText: 'Tasks' })).toBeVisible();
  md += '\n\n## Tasks\n\n- [ ] Order seeds';
  await saved(yap, n.id, md);

  // The toolbar: a word made code.
  const beans = md.indexOf('Beans');
  await selectText(raw(main), beans, beans + 'Beans'.length);
  await tools(main).getByRole('button', { name: 'Code' }).click();
  await expect(tools(main).getByRole('button', { name: 'Code' })).toHaveAttribute('aria-pressed', 'true');
  md = md.replace('Beans in March', '`Beans` in March');
  await saved(yap, n.id, md);
  // Text style ▸ Heading 1 on "Tasks".
  await selectText(raw(main), md.indexOf('Tasks') + 2);
  await tools(main).getByRole('button', { name: /^Text style/ }).click();
  await expect(main.getByRole('menuitemradio', { name: 'Heading 2' })).toHaveAttribute('aria-checked', 'true');
  await main.getByRole('menuitemradio', { name: 'Heading 1' }).click();
  await expect(raw(main).locator('h2', { hasText: 'Tasks' })).toBeVisible();
  md = md.replace('## Tasks', '# Tasks');
  await saved(yap, n.id, md);
  // Ctrl+Z, the page's own undo: back as it was.
  await main.keyboard.press('Control+z');
  md = md.replace('# Tasks', '## Tasks');
  await saved(yap, n.id, md);

  // A box ticked on the page: its line, in place. (Where the bullet would be, inside the page: an answer's -18px
  // from app.css once put it past the page's edge, half hidden and out of a click's reach.)
  const box = raw(main).locator('li.task', { hasText: 'Order seeds' }).locator('input.taskbox');
  const [boxAt, pageAt] = await Promise.all([box.boundingBox(), raw(main).boundingBox()]);
  expect(boxAt.x).toBeGreaterThanOrEqual(pageAt.x);
  await box.click();
  await expect(box).toBeChecked();
  md = md.replace('- [ ] Order seeds', '- [x] Order seeds');
  await saved(yap, n.id, md);
  await shot(main, '02-formatted-and-ticked');

  // Version history: the typing, folded into one typed version, saying it all.
  await expect.poll(async () => (await versions(yap, n.id))[0]?.text).toBe(md);
  expect((await versions(yap, n.id))[0].kind).toBe('typed');
  await main.getByRole('button', { name: 'More for this note' }).click();
  await main.getByRole('menu').getByRole('menuitem', { name: 'Version history…' }).click();
  const dialog = main.getByRole('dialog', { name: 'Version history' });
  await expect(dialog.getByRole('option').first().locator('.ikind')).toHaveText('Typed');
  await main.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
});

test('Notes: Preview / Source — the markdown in a plain box, edited there, remembered; what the page can\'t hold kept whole', async ({ yap, main, shot }) => {
  const n = await yap.invoke('note_create', { title: 'Reading list', content: '- # A heading in a list\n- second\n\nThe rest' });
  await yap.invoke('note_create', { title: 'Another note', content: 'Plain words' });
  await openNote(main, 'Reading list');
  // A list item that starts with a heading: kept as it is, read-only, edited in Source.
  const kept = raw(main).locator('.docraw');
  await expect(kept).toHaveAttribute('title', 'Edit this part in Source');
  await expect(kept).toHaveAttribute('contenteditable', 'false');

  // Source: the markdown, exactly; the toolbar off.
  await main.getByRole('group', { name: 'View' }).getByRole('button', { name: 'Source' }).click();
  const src = raw(main);
  await expect(src).toHaveJSProperty('tagName', 'TEXTAREA');
  await expectText(src, '- # A heading in a list\n- second\n\nThe rest');
  await expect(tools(main).getByRole('button', { name: 'Code' })).toBeDisabled();
  await shot(main, '03-source');
  await selectText(src, '- # A heading in a list'.length);
  await main.keyboard.type(' (new)');
  await saved(yap, n.id, '- # A heading in a list (new)\n- second\n\nThe rest');

  // Remembered: the next note opens on Source too.
  await noteRow(main, 'Another note').click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Another note');
  await expect(raw(main)).toHaveJSProperty('tagName', 'TEXTAREA');
  await expectText(raw(main), 'Plain words');
  // Preview again: the page, with Source's edit.
  await noteRow(main, 'Reading list').click();
  await main.getByRole('group', { name: 'View' }).getByRole('button', { name: 'Preview' }).click();
  await pageIn(raw(main));
  await expect(raw(main).locator('.docraw')).toContainText('(new)');
  await expect(tools(main).getByRole('button', { name: 'Code' })).toBeEnabled();
});

test("Notes: Yap's paste of a dictation goes in at the caret as plain text, a spoken list a list; its version Dictated", async ({ yap, main }) => {
  const n = await yap.invoke('note_create', { title: 'Call notes', content: 'Agenda' });
  await openNote(main, 'Call notes');
  await selectText(raw(main), 'Agenda'.length);
  // Yap delivered a dictation (`yap-transcript`; a test run pastes nothing)…
  await yap.invoke('e2e_dictation', { text: 'for Friday' });
  // …and its paste lands at the caret.
  await paste(raw(main), ' for Friday');
  await expectText(raw(main), 'Agenda for Friday');
  await saved(yap, n.id, 'Agenda for Friday');
  await expect.poll(async () => (await versions(yap, n.id))[0]?.kind).toBe('dictated');

  // A spoken list the cleanup laid out: a list on the page, after a line of its own.
  await selectText(raw(main), 'Agenda for Friday'.length);
  await main.keyboard.press('Enter');
  await paste(raw(main), 'Bring:\n1. Tickets\n2. Snacks');
  await expect(raw(main).locator('ol > li')).toHaveCount(2);
  await saved(yap, n.id, 'Agenda for Friday\n\nBring:\n\n1. Tickets\n2. Snacks');
  // One line's markup stays its words.
  const end = (await noteText(raw(main))).length;
  await selectText(raw(main), end);
  await main.keyboard.press('Enter');
  await main.keyboard.press('Enter');
  await paste(raw(main), '# not a heading');
  await expect(raw(main).locator('h2')).toHaveCount(0);
  await saved(yap, n.id, 'Agenda for Friday\n\nBring:\n\n1. Tickets\n2. Snacks\n\n\\# not a heading');
});

test('the meeting notepad: My thoughts typed and formatted while a test meeting records, synced with Notes', async ({ yap, main, shot }) => {
  const note = await yap.invoke('note_create', { title: 'Supplier call', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await expect.poll(() => recording(yap)).toBe(true);
  const pad = yap.notepad;
  const thoughts = raw(pad);
  await pageIn(thoughts);
  await expect(tools(pad)).toBeVisible();
  // The placeholder says what to do while it records.
  await expect(thoughts.locator('p.is-editor-empty')).toHaveAttribute('data-placeholder', 'Jot down your own notes while Yap listens. They go into the summary.');

  await thoughts.click();
  await pad.keyboard.type('## Questions');
  await pad.keyboard.press('Enter');
  await pad.keyboard.type('- Delivery dates?');
  await pad.keyboard.press('Enter');
  await pad.keyboard.type('Price per crate');
  // The toolbar: the second item a checklist one.
  await tools(pad).getByRole('button', { name: 'Checklist' }).click();
  const md = '## Questions\n\n- Delivery dates?\n- [ ] Price per crate';
  await saved(yap, note.id, md);
  await expect(thoughts.locator('li.task', { hasText: 'Price per crate' })).toBeVisible();
  await shot(pad, '04-notepad-formatted-while-recording');
  expect(await recording(yap)).toBe(true);

  // The Notes view follows, as the same page.
  await openNote(main, 'Supplier call', { folder: 'Meetings' });
  await expectText(raw(main), md);
  await expect(raw(main).locator('li.task', { hasText: 'Price per crate' })).toBeVisible();
  // Ticked there: the notepad follows.
  await raw(main).locator('li.task', { hasText: 'Price per crate' }).locator('input.taskbox').click();
  await saved(yap, note.id, md.replace('- [ ]', '- [x]'));
  await expect(thoughts.locator('li.task', { hasText: 'Price per crate' }).locator('input.taskbox')).toBeChecked();
  await expectText(thoughts, md.replace('- [ ]', '- [x]'));
});
