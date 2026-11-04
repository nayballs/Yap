// Settings → Memory → "Import a file…" (desktop/src-tauri/src/memory/import.rs) and
// "Copy for Claude", with no AI model set up at all: a file import is Yap's
// own reading, so nothing goes to a model. A made-up Claude memory export
// (a .zip built here, fictional people) is picked (the dialog's path named
// by `e2e_memory_file`), its review shows what's new and what merges into
// what Yap already has, nothing is kept until Save, the items land in
// Settings from Claude, and one Undo takes all of it back; a topic keeps
// its last eight facts and the review says which didn't fit. The export
// unzipped, dropped on the page (as Tauri reports a drop) over Chat, is an
// import Chat leaves alone; Yap's own export comes back as it was; and Copy
// for Claude puts memory on the clipboard as plain text (a test run's
// pretend clipboard: the real one is never touched).
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { test, expect, openSettings, closeSettings, openView, expectStore } from './support/fixtures.js';

test.use({
  yapOptions: {
    name: 'memory-file',
    // Windows' region as a test run has it (ai/about_you.rs): the UK.
    env: {
      YAP_E2E_LOCALE: JSON.stringify({ locale: 'en-GB', country: 'GB', currency: 'GBP', symbol: '£', metric: true, timeZone: 'GMT Standard Time' }),
    },
  },
});

/** A .zip of `files` ({name: text}), Deflate, as Claude's export is. */
function zipOf(files) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const [name, text] of Object.entries(files)) {
    const nameBuf = Buffer.from(name, 'utf8');
    const data = Buffer.from(text, 'utf8');
    const packed = zlib.deflateRawSync(data);
    const crc = zlib.crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0x0800, 6); // UTF-8 names
    local.writeUInt16LE(8, 8); // Deflate
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(packed.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    locals.push(local, nameBuf, packed);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt16LE(0x0800, 8);
    entry.writeUInt16LE(8, 10);
    entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(packed.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(nameBuf.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, nameBuf);
    offset += 30 + nameBuf.length + packed.length;
  }
  const dir = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10);
  end.writeUInt32LE(dir.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, dir, end]);
}

const md = (name, description, facts) =>
  `---\nname: "${name}"\ndescription: "${description}"\n---\n\n${facts.map((f) => `- ${f}\n`).join('')}`;

/** A made-up Claude memory export. */
const ROOT = 'claude-memory-2026-11-03';
const EXPORT = {
  [`${ROOT}/preferences.md`]: md('preferences', 'How they like answers', ['Prefers short answers']),
  [`${ROOT}/profile.md`]: md('profile', 'Core facts', ['Lives in Harrogate', 'Works as a garden designer']),
  [`${ROOT}/people/sam-reed.md`]: md('sam-reed', "Sam's my brother-in-law; lives in Leeds", ['Birthday 14 March', 'Works as a structural engineer']),
  [`${ROOT}/people/priya-shah.md`]: md('priya-shah', 'Priya Shah is my manager at Acme', ['Runs the design team']),
  [`${ROOT}/topics/gardening.md`]: md('gardening', 'Growing vegetables at home', ['Grows tomatoes in a greenhouse']),
  [`${ROOT}/areas/allotment-plan.md`]: md('allotment-plan', 'The plan for the allotment', ['Plot 14 at the Westfield site']),
  [`${ROOT}/projects/Kitchen refit/topics/suppliers.md`]: md('suppliers', 'Worktop suppliers being compared', ['Oak from a supplier in York']),
  // Ten facts: a topic keeps its last eight.
  [`${ROOT}/topics/chess.md`]: md('chess', 'A game they play', [
    'Plays online',
    'Likes the Sicilian',
    'Rated about 1500',
    'Plays on Sundays',
    'Reads chess books',
    'Follows the world championship',
    'Has a wooden set',
    'Studies endgames',
    'Joined a club',
    'Teaches a niece',
  ]),
};

const items = (yap) => yap.readJson('memory.json')?.items ?? [];
const toastWith = (main, text) => main.getByRole('status').filter({ hasText: text });

test("Claude's memory export: reviewed with what Yap has, kept on Save, undone in one go", async ({ yap, main, shot }) => {
  // What Yap already remembers: Sam (his birthday) and where you live.
  await yap.invoke('memory_clear');
  await yap.invoke('memory_save_item', { item: { section: 'people', title: 'Sam', summary: '', details: ['Birthday 14 March'] } });
  await yap.invoke('memory_save_item', { item: { section: 'profile', title: '', summary: '', details: ['Lives in Harrogate'] } });
  const file = path.join(yap.runDir, 'claude-memory-2026-11-03.zip');
  fs.writeFileSync(file, zipOf(EXPORT));

  const dialog = await openSettings(main, 'Memory');
  // No AI model here at all, and the file import still works.
  await expect(dialog.getByText('Yap needs an AI model for memory')).toBeVisible();
  const row = dialog.locator('[data-setting="memory-import-file"]');
  await expect(row).toContainText('Read on this PC: nothing is sent anywhere');
  await yap.invoke('e2e_memory_file', { path: file });
  await row.getByRole('button', { name: 'Import a file…' }).click();

  const review = dialog.getByRole('region', { name: 'Import a file: review' });
  await expect(review).toContainText('From Claude’s memory export');
  const diff = review.getByRole('list', { name: 'Proposed changes' });
  // Eight things in the file; Sam and Profile merge, so only what's new of them.
  await expect(diff.getByRole('checkbox')).toHaveCount(8);
  // What didn't fit is said above, and struck out on its item.
  await expect(review.getByRole('list', { name: 'Left out' })).toContainText('2 facts didn’t fit');
  const chess = diff.getByRole('listitem').filter({ hasText: 'Chess' });
  await expect(chess.locator('s.left-out')).toHaveText(['Plays online', 'Likes the Sicilian']);
  await expect(chess.locator('ins').filter({ hasText: 'Teaches a niece' })).toHaveCount(1);
  const sam = diff.getByRole('listitem').filter({ hasText: 'Sam Reed' });
  await expect(sam).toContainText('Works as a structural engineer');
  await expect(sam).toContainText("Sam's my brother-in-law; lives in Leeds");
  await expect(sam.locator('ins').filter({ hasText: 'Birthday 14 March' })).toHaveCount(0);
  const profile = diff.getByRole('listitem').filter({ hasText: 'Works as a garden designer' });
  await expect(profile.locator('ins').filter({ hasText: 'Lives in Harrogate' })).toHaveCount(0);
  await expect(diff).toContainText('Priya Shah');
  await expect(diff).toContainText('Allotment plan');
  await expect(diff).toContainText('Worktop suppliers being compared (Claude Project “Kitchen refit”)');
  // Nothing is kept before Save.
  expect(items(yap)).toHaveLength(2);
  await review.scrollIntoViewIfNeeded();
  await shot(main, '01-review');

  // Leave gardening out, keep the rest.
  await diff.getByRole('checkbox', { name: 'Keep Gardening' }).uncheck();
  await review.getByRole('button', { name: 'Save 7' }).click();
  await expectStore(yap, 'memory.json', (m) => {
    const by = (t) => m.items.find((i) => i.title === t);
    return (
      by('Priya Shah')?.source?.kind === 'import' &&
      by('Priya Shah')?.source?.title === 'Claude' &&
      by('Sam Reed')?.details?.join('|') === 'Birthday 14 March|Works as a structural engineer' &&
      by('Suppliers')?.section === 'topics' &&
      by('Allotment plan')?.section === 'projects' &&
      by('Chess')?.details?.length === 8 &&
      !by('Gardening') &&
      m.items.some((i) => i.section === 'profile' && i.details.join('|') === 'Lives in Harrogate|Works as a garden designer')
    );
  });
  await expect(review).toHaveCount(0);

  // In Settings, from Claude.
  const people = dialog.getByRole('group', { name: 'People', exact: true });
  await expect(people.getByRole('button', { name: /^Sam Reed/ })).toBeVisible();
  await people.getByRole('button', { name: /^Priya Shah/ }).click();
  await expect(people.getByRole('group', { name: 'Edit Priya Shah' })).toContainText('Imported from Claude');
  await shot(main, '02-imported');

  // One Undo: all of it, back to what was there.
  const updated = toastWith(main, 'Memory updated: 7 changes');
  await expect(updated).toBeVisible();
  await updated.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.length === 2 && m.items.some((i) => i.title === 'Sam' && i.details.length === 1));
  await expect(people.getByRole('button', { name: /^Priya Shah/ })).toHaveCount(0);

  // The same file once kept: nothing new the second time.
  await yap.invoke('e2e_memory_file', { path: file });
  await row.getByRole('button', { name: 'Import a file…' }).click();
  await review.getByRole('button', { name: 'Save 8' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.length === 8);
  await yap.invoke('e2e_memory_file', { path: file });
  await row.getByRole('button', { name: 'Import a file…' }).click();
  await expect(review).toContainText('Nothing new in Claude’s memory export');
  await review.getByRole('button', { name: 'Close' }).click();
  await closeSettings(main);
});

test("a dropped folder, Yap's own export, and a file that isn't memory", async ({ yap, main }) => {
  // The export unzipped, as a folder dropped on Settings → Memory over Chat
  // (as Tauri reports a drop): an import, which Chat leaves alone.
  await yap.invoke('memory_clear');
  const dir = path.join(yap.runDir, 'unzipped', ROOT);
  for (const [name, text] of Object.entries(EXPORT)) {
    const p = path.join(yap.runDir, 'unzipped', ...name.split('/'));
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, text);
  }
  await openView(main, 'Chat');
  const dialog = await openSettings(main, 'Memory');
  const review = dialog.getByRole('region', { name: 'Import a file: review' });
  await expect(async () => {
    await yap.invoke('plugin:event|emit', { event: 'tauri://drag-drop', payload: { paths: [dir], position: { x: 600, y: 400 } } });
    await expect(review).toBeVisible({ timeout: 1_000 });
  }).toPass();
  await expect(review).toContainText('From Claude’s memory export');
  await expect(review.getByRole('checkbox')).toHaveCount(8);
  await expect(toastWith(main, "weren't added")).toHaveCount(0);
  await review.getByRole('button', { name: 'Save 8' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.length === 8);

  // Yap's own export, into an empty memory (another PC): the same items back.
  const backup = path.join(yap.runDir, 'yap-memory.zip');
  await yap.invoke('memory_export', { path: backup });
  const before = items(yap).map((i) => [i.section, i.title, i.summary, i.details.join('|')].join('/')).sort();
  await yap.invoke('memory_clear');
  await yap.invoke('e2e_memory_file', { path: backup });
  await dialog.getByRole('button', { name: 'Import a file…' }).click();
  await expect(review).toContainText('From a Yap memory export');
  await expect(review.getByRole('list', { name: 'Left out' })).toContainText('About you');
  await review.getByRole('button', { name: 'Save 8' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.length === 8);
  expect(items(yap).map((i) => [i.section, i.title, i.summary, i.details.join('|')].join('/')).sort()).toEqual(before);
  expect(items(yap).every((i) => i.source.title === 'a Yap export')).toBe(true);

  // Not memory: said, nothing changes.
  const notes = path.join(yap.runDir, 'notes.txt');
  fs.writeFileSync(notes, 'just some text');
  await yap.invoke('e2e_memory_file', { path: notes });
  await dialog.getByRole('button', { name: 'Import a file…' }).click();
  await expect(toastWith(main, "Yap couldn't import that")).toContainText("Claude's memory export");
  await closeSettings(main);
});

test('Copy for Claude: memory as plain text, a line a fact', async ({ yap, main, shot }) => {
  await yap.invoke('memory_clear');
  await yap.invoke('memory_save_item', { item: { section: 'preferences', title: '', summary: '', details: ['Prefers short answers'] } });
  await yap.invoke('memory_save_item', { item: { section: 'people', title: 'Sam Reed', summary: 'Brother-in-law', details: ['Lives in Leeds'] } });
  await yap.invoke('memory_save_item', { item: { section: 'topics', title: 'Chess', summary: '', details: ['Plays online'] } });
  const dialog = await openSettings(main, 'Memory');
  await yap.invoke('e2e_memory_clipboard');
  const copy = dialog.getByRole('button', { name: 'Copy for Claude' });
  await copy.scrollIntoViewIfNeeded();
  await copy.click();
  await expect(toastWith(main, 'Copied')).toContainText('Start import');
  await shot(main, '03-copied');
  const text = await yap.invoke('e2e_memory_clipboard');
  // About you first (the region from Windows), then the sections, a line a fact.
  expect(text).toMatch(/^About you\n- Region: /);
  expect(text).toContain('\n\nPreferences\n- Prefers short answers\n');
  expect(text).toContain('\n\nPeople\n- Sam Reed: Brother-in-law\n- Sam Reed: Lives in Leeds\n');
  expect(text).toContain('\n\nTopics\n- Chess: Plays online\n');
  expect(text).not.toContain('Profile');
  await closeSettings(main);
});
