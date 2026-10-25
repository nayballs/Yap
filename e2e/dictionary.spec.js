// The Dictionary view for a dictionary of hundreds (2026-10-25): a search
// that filters as you type (Ctrl+F; Settings' own when Settings is open),
// stars and the four sorts (remembered), checkboxes with Shift-click ranges
// and Ctrl+A, the selection's bar (≈ on or off, Delete… asked first and
// undone), Import… with its preview (what's added, already there, spelled
// differently, left out) and its Undo, Export… as CSV or text, and 2,000
// entries staying quick. A test run never opens Windows' file dialogs:
// `e2e_dictionary_file` names the file the next Import… reads or Export…
// writes. The parsing and sorting rules are e2e/dictionary-list.spec.js's.
// Every word here is made up or an everyday product name.
import fs from 'node:fs';
import path from 'node:path';
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  setConfig,
} from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);

const DICT = [
  { from: 'power to keep', to: 'Parakeet', fuzzy: true, snippet: false },
  { from: 'cube and eighties', to: 'Kubernetes', fuzzy: true, snippet: false },
  { from: 'zen tricks', to: 'Zentrix', fuzzy: true, snippet: false },
  { from: 'jaison', to: 'JSON', fuzzy: false, snippet: false },
  { from: 'vex well', to: 'Vexwell', fuzzy: true, snippet: false },
  { from: 'my sign-off', to: 'Speak soon,\nRobin', fuzzy: true, snippet: true },
  { from: 'my address', to: '1 Long Lane\nLeeds', fuzzy: true, snippet: true },
];
const WORDS = DICT.filter((e) => !e.snippet).map((e) => e.to);

const test = base.extend({
  yapOptions: [
    {
      name: 'dictionary',
      config: { dictionary: DICT },
      // How often each word's been used (dict_usage.rs): "Most used".
      data: {
        'dictionary_usage.json': {
          json: { uses: 30, lastUsed: NOW - 50 },
          kubernetes: { uses: 12, lastUsed: NOW - 100 },
          zentrix: { uses: 3, lastUsed: NOW - 10 },
        },
      },
    },
    { scope: 'worker' },
  ],
});

const corrections = (main) => main.getByRole('region', { name: 'Corrections' });
const snippets = (main) => main.getByRole('region', { name: 'Snippets' });
const searchBox = (main) => main.getByRole('searchbox', { name: 'Search the dictionary' });
const sortBox = (main) => main.getByRole('combobox', { name: 'Sort the dictionary' });
const bulk = (main) => main.getByRole('toolbar', { name: 'Selected entries' });
const check = (main, word) => main.getByRole('checkbox', { name: `Select “${word}”`, exact: true });
const row = (main, word) => corrections(main).locator('.dict-row', { has: check(main, word) });
/** The words in the Corrections list, top to bottom. */
const words = (main) => corrections(main).getByRole('textbox', { name: 'Replace with' }).evaluateAll((els) => els.map((el) => el.value));
/** The snippets' phrases, top to bottom. */
const phrases = (main) => snippets(main).getByRole('textbox', { name: 'Phrase you say' }).evaluateAll((els) => els.map((el) => el.value));
const toastWith = (main, text) => main.getByRole('status').filter({ hasText: text });

/** The dictionary as `list`, the sort forgotten, then the view. */
async function freshView(yap, main, list = DICT) {
  await main.setViewportSize({ width: 1200, height: 800 });
  await setConfig(yap, main, { dictionary: list });
  await main.evaluate(() => localStorage.removeItem('yap.dictionarySort'));
  await openView(main, 'Dictionary');
  await expect(main.getByRole('heading', { level: 1, name: 'Dictionary' })).toBeVisible();
}

test('search filters as you type, across words, how they are heard and snippets; Ctrl+F finds it unless Settings is open', async ({
  yap,
  main,
  shot,
}) => {
  await freshView(yap, main);
  await expect.poll(() => words(main)).toEqual(WORDS);
  await main.getByRole('heading', { level: 1, name: 'Dictionary' }).click();
  await main.keyboard.press('Control+f');
  await expect(searchBox(main)).toBeFocused();

  // The word: every way it's heard.
  await main.keyboard.type('kube');
  await expect.poll(() => words(main)).toEqual(['Kubernetes']);
  await expect(snippets(main).getByText('No snippets match.')).toBeVisible();
  await expect(main.getByText('1 entry of 7 match “kube”')).toBeVisible();
  await shot(main, '01-search');
  // How it's heard.
  await searchBox(main).fill('eighties');
  await expect.poll(() => words(main)).toEqual(['Kubernetes']);
  // A snippet's text, any case.
  await searchBox(main).fill('ROBIN');
  await expect.poll(() => words(main)).toEqual([]);
  await expect.poll(() => phrases(main)).toEqual(['my sign-off']);
  await expect(corrections(main).getByText('No corrections match.')).toBeVisible();
  // Nothing.
  await searchBox(main).fill('zzz');
  await expect(main.getByText('Nothing matches “zzz”.')).toBeVisible();
  // Esc in the box clears it.
  await searchBox(main).press('Escape');
  await expect(searchBox(main)).toHaveValue('');
  await expect.poll(() => words(main)).toEqual(WORDS);
  await expect.poll(() => phrases(main)).toEqual(['my sign-off', 'my address']);

  // A row added while searching shows (at the bottom) whatever the search.
  await searchBox(main).fill('kube');
  await corrections(main).getByRole('button', { name: '+ Add correction' }).click();
  await expect(corrections(main).getByRole('textbox', { name: 'Heard' }).last()).toBeFocused();
  await expect.poll(() => words(main)).toEqual(['Kubernetes', '']);
  await searchBox(main).fill('');

  // Settings open: Ctrl+F is its search, not the page's.
  const dialog = await openSettings(main, 'General');
  await main.keyboard.press('Control+f');
  await expect(dialog.getByRole('searchbox', { name: 'Search settings' })).toBeFocused();
  await expect(searchBox(main)).not.toBeFocused();
  await closeSettings(main);
  await main.getByRole('heading', { level: 1, name: 'Dictionary' }).click();
  await main.keyboard.press('Control+f');
  await expect(searchBox(main)).toBeFocused();
});

test('star and sort: starred first, A–Z, most used, recently added; the sort is remembered and the star saved', async ({
  yap,
  main,
  shot,
}) => {
  await freshView(yap, main);
  await expect(sortBox(main)).toHaveValue('starred');

  // A star: saved, and the order stays put until the list is sorted again.
  const star = row(main, 'Vexwell').getByRole('button', { name: 'Star' });
  await expect(star).toHaveAttribute('aria-pressed', 'false');
  await star.click();
  await expect(star).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'config.json', (c) => c.dictionary.find((e) => e.to === 'Vexwell')?.starred === true);
  // Written only where set.
  expect(yap.readConfig().dictionary.filter((e) => 'starred' in e).map((e) => e.to)).toEqual(['Vexwell']);
  await expect.poll(() => words(main)).toEqual(WORDS);

  await sortBox(main).selectOption('az');
  await expect.poll(() => words(main)).toEqual(['JSON', 'Kubernetes', 'Parakeet', 'Vexwell', 'Zentrix']);
  await expect.poll(() => phrases(main)).toEqual(['my address', 'my sign-off']);
  await sortBox(main).selectOption('starred');
  await expect.poll(() => words(main)).toEqual(['Vexwell', 'Parakeet', 'Kubernetes', 'Zentrix', 'JSON']);
  await sortBox(main).selectOption('used');
  await expect.poll(() => words(main)).toEqual(['JSON', 'Kubernetes', 'Zentrix', 'Parakeet', 'Vexwell']);
  await expect(row(main, 'JSON').locator('.uses')).toHaveText('30×');
  await expect(row(main, 'Parakeet').locator('.uses')).toHaveText('–');
  await shot(main, '02-most-used');
  await sortBox(main).selectOption('recent');
  await expect.poll(() => words(main)).toEqual(['Vexwell', 'JSON', 'Zentrix', 'Kubernetes', 'Parakeet']);
  await expect.poll(() => phrases(main)).toEqual(['my address', 'my sign-off']);

  // Remembered: a reload, then back to the view.
  await main.reload({ waitUntil: 'load' });
  await openView(main, 'Dictionary');
  await expect(sortBox(main)).toHaveValue('recent');
  await expect.poll(() => words(main)).toEqual(['Vexwell', 'JSON', 'Zentrix', 'Kubernetes', 'Parakeet']);

  // Settings auto-saves its own copy of the config: the star stays.
  const dialog = await openSettings(main, 'Dictation');
  const space = dialog.getByRole('button', { name: 'Append trailing space' });
  const before = await space.getAttribute('aria-pressed');
  await space.click();
  await expectStore(yap, 'config.json', (c) => String(c.appendTrailingSpace) !== before);
  await space.click();
  await expectStore(yap, 'config.json', (c) => String(c.appendTrailingSpace) === before);
  expect(yap.readConfig().dictionary.find((e) => e.to === 'Vexwell').starred).toBe(true);
  await closeSettings(main);

  // The row's right-click menu stars too (here: unstars).
  await row(main, 'Vexwell').locator('.arrow').click({ button: 'right' });
  const item = main.getByRole('menu').getByRole('menuitemcheckbox', { name: 'Star' });
  await expect(item).toHaveAttribute('aria-checked', 'true');
  await item.click();
  await expectStore(yap, 'config.json', (c) => !c.dictionary.some((e) => 'starred' in e));
});

test('select several: Shift-click ranges, Ctrl+A, ≈ for the selection, Delete… asked first and undone', async ({
  yap,
  main,
  shot,
}) => {
  await freshView(yap, main);
  await check(main, 'Parakeet').click();
  await check(main, 'Zentrix').click({ modifiers: ['Shift'] });
  await expect(bulk(main)).toContainText('3 entries selected');
  for (const w of ['Parakeet', 'Kubernetes', 'Zentrix']) await expect(check(main, w)).toBeChecked();
  await expect(check(main, 'JSON')).not.toBeChecked();
  await shot(main, '03-selected');

  // ≈ off then on for the three; JSON (off already) untouched.
  await bulk(main).getByRole('button', { name: 'Exact only' }).click();
  await expectStore(yap, 'config.json', (c) =>
    c.dictionary.filter((e) => !e.snippet).map((e) => e.fuzzy).join() === 'false,false,false,false,true'
  );
  await expect(row(main, 'Kubernetes').locator('.fz')).not.toHaveClass(/\bon\b/);
  await bulk(main).getByRole('button', { name: '≈ Catch near-misses' }).click();
  await expectStore(yap, 'config.json', (c) =>
    c.dictionary.filter((e) => !e.snippet).map((e) => e.fuzzy).join() === 'true,true,true,false,true'
  );

  // Ctrl+A in the list: everything shown, snippets too. Esc: none.
  await check(main, 'JSON').focus();
  await main.keyboard.press('Control+a');
  await expect(bulk(main)).toContainText('7 entries selected');
  await main.keyboard.press('Escape');
  await expect(bulk(main)).toBeHidden();
  // A section's own box: every correction shown.
  await corrections(main).getByRole('checkbox', { name: 'Select every correction shown' }).click();
  await expect(bulk(main)).toContainText('5 entries selected');
  await bulk(main).getByRole('button', { name: 'Clear the selection' }).click();
  await expect(bulk(main)).toBeHidden();

  // Two, one of them a snippet: Delete… asks, then they go, with Undo.
  await check(main, 'Kubernetes').click();
  await main.getByRole('checkbox', { name: 'Select the snippet “my sign-off”' }).click();
  await bulk(main).getByRole('button', { name: 'Delete…' }).click();
  const ask = main.getByRole('alertdialog', { name: 'Delete 2 entries?' });
  await expect(ask).toContainText('1 correction and 1 snippet go from your dictionary. You can undo it straight after.');
  await shot(main, '04-delete-ask');
  await ask.getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionary.length === 5 && !c.dictionary.some((e) => e.to === 'Kubernetes' || e.from === 'my sign-off'));
  await expect.poll(() => words(main)).toEqual(['Parakeet', 'Zentrix', 'JSON', 'Vexwell']);
  const done = toastWith(main, 'Deleted 2 entries');
  await expect(done).toBeVisible();
  await shot(main, '05-deleted-undo');
  await done.getByRole('button', { name: 'Undo' }).click();
  // Back where they were.
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.dictionary) === JSON.stringify(DICT));
  await expect.poll(() => words(main)).toEqual(WORDS);
  await expect.poll(() => phrases(main)).toEqual(['my sign-off', 'my address']);

  // The Delete key on a selection asks too; Cancel keeps it.
  await check(main, 'JSON').click();
  await main.keyboard.press('Delete');
  const one = main.getByRole('alertdialog', { name: 'Delete “JSON”?' });
  await expect(one).toBeVisible();
  await one.getByRole('button', { name: 'Cancel' }).click();
  await expect(one).toBeHidden();
  expect(yap.readConfig().dictionary).toHaveLength(7);

  // Undo still works after leaving the view: on config.json.
  await closeToasts(main);
  await row(main, 'Vexwell').getByRole('button', { name: 'Remove' }).click();
  await main.getByRole('alertdialog', { name: 'Delete “Vexwell”?' }).getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'config.json', (c) => !c.dictionary.some((e) => e.to === 'Vexwell'));
  await openView(main, 'Home');
  await toastWith(main, 'Deleted “Vexwell”').getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.dictionary) === JSON.stringify(DICT));
});

test('import: the preview says what is added, already there, spelled differently and left out; the choices apply, with Undo', async ({
  yap,
  main,
  shot,
}) => {
  await freshView(yap, main);
  const file = path.join(yap.runDir, 'my words.csv');
  fs.writeFileSync(
    file,
    [
      'Heard,Written,Kind,Near-misses,Starred',
      'power to keep,Parakeet,correction,yes,', // already there
      'jaison,Json,correction,yes,', // spelled differently
      '"quinn, the app",Quinnly,correction,no,yes', // new: a comma inside quotes, exact only, starred
      'orbit lee,Orbitly,correction,yes,', // new (unticked below)
      'Vexwell,Vexwell,correction,,', // the word alone: already there
      'my meeting link,"Join here:\r\nhttps://example.com/room",snippet,,', // a new snippet, two lines
      'Orbit Lee,Orbitly,correction,,', // said again
      ',,,,',
      'my address,,snippet,,', // a snippet with no text: left out
    ].join('\r\n')
  );
  await yap.invoke('e2e_dictionary_file', { path: file });
  await main.getByRole('button', { name: 'Import…' }).click();
  const dialog = main.getByRole('dialog', { name: 'Import from “my words.csv”' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('2 corrections and 1 snippet to add · 1 spelled differently · 2 already in your dictionary · 2 left out.');
  const add = dialog.getByRole('button', { name: 'Add 3 entries' });
  await expect(add).toBeFocused();
  await shot(main, '06-import-preview');

  // Leave one out; take the file's spelling for the conflict.
  await dialog.getByRole('checkbox', { name: 'orbit lee → Orbitly' }).uncheck();
  await dialog.getByRole('radio', { name: "Use the file's: Json" }).check();
  await expect(dialog.getByRole('button', { name: 'Add 2, change 1' })).toBeVisible();
  // What's left out says why.
  await dialog.locator('summary', { hasText: 'Left out' }).click();
  await expect(dialog).toContainText('Line 11: “my address,,snippet” — A snippet needs the phrase you say and its text.');
  await expect(dialog).toContainText('Line 9: “Orbit Lee → Orbitly” — said again earlier in the file.');
  await dialog.getByRole('button', { name: 'Add 2, change 1' }).click();
  await expect(dialog).toBeHidden();
  await expect(main.getByRole('button', { name: 'Import…' })).toBeFocused();

  await expectStore(yap, 'config.json', (c) => {
    const by = (from) => c.dictionary.find((e) => e.from === from);
    return (
      c.dictionary.length === 9 &&
      by('jaison')?.to === 'Json' &&
      by('quinn, the app')?.fuzzy === false &&
      by('quinn, the app')?.starred === true &&
      by('my meeting link')?.snippet === true &&
      by('my meeting link')?.to === 'Join here:\nhttps://example.com/room' &&
      !by('orbit lee')
    );
  });
  await expect.poll(() => words(main)).toEqual(['Quinnly', 'Parakeet', 'Kubernetes', 'Zentrix', 'Json', 'Vexwell']);
  const done = toastWith(main, 'Added 2 entries, changed 1');
  await expect(done).toContainText('From “my words.csv”.');
  await done.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'config.json', (c) => JSON.stringify(c.dictionary) === JSON.stringify(DICT));
  await closeToasts(main);

  // Plain text: "heard -> written" and words alone; Cancel changes nothing.
  const text = path.join(yap.runDir, 'words.txt');
  fs.writeFileSync(text, '# from my notes\ncuber netties -> Kubernetes\nOrbitly\n\nZentrix\n');
  await yap.invoke('e2e_dictionary_file', { path: text });
  await main.getByRole('button', { name: 'Import…' }).click();
  const second = main.getByRole('dialog', { name: 'Import from “words.txt”' });
  await expect(second).toContainText('2 entries to add · 1 already in your dictionary.');
  await expect(second).toContainText('Read as text, one entry a line.');
  await second.getByRole('button', { name: 'Cancel' }).click();
  await expect(second).toBeHidden();
  // No file named for this one: the test run's stand-in for the dialog says so.
  await main.getByRole('button', { name: 'Import…' }).click();
  await expect(toastWith(main, "Couldn't read that file")).toBeVisible();
  expect(JSON.stringify(yap.readConfig().dictionary)).toBe(JSON.stringify(DICT));
});

test('export: everything as CSV, the selection as text', async ({ yap, main }) => {
  await freshView(yap, main, DICT.map((e) => (e.to === 'Vexwell' ? { ...e, starred: true } : e)));
  const csv = path.join(yap.runDir, 'export.csv');
  fs.rmSync(csv, { force: true });
  await yap.invoke('e2e_dictionary_file', { path: csv });
  await main.locator('.toolrow').getByRole('button', { name: 'Export…' }).click();
  await main.getByRole('menuitem', { name: 'As CSV…' }).click();
  await expect(toastWith(main, 'Exported 7 entries')).toContainText('Saved as “export.csv”.');
  const written = fs.readFileSync(csv, 'utf8');
  expect(written.split('\r\n').slice(0, 3)).toEqual([
    '﻿Heard,Written,Kind,Near-misses,Starred',
    'power to keep,Parakeet,correction,yes,',
    'cube and eighties,Kubernetes,correction,yes,',
  ]);
  expect(written).toContain('jaison,JSON,correction,no,\r\n');
  expect(written).toContain('vex well,Vexwell,correction,yes,yes\r\n');
  expect(written).toContain('my sign-off,"Speak soon,\nRobin",snippet,,\r\n');

  // The selection, as text: snippets need CSV, so they're left out (and said).
  await closeToasts(main);
  const txt = path.join(yap.runDir, 'export.txt');
  fs.rmSync(txt, { force: true });
  await check(main, 'Parakeet').click();
  await check(main, 'JSON').click();
  await main.getByRole('checkbox', { name: 'Select the snippet “my address”' }).click();
  await yap.invoke('e2e_dictionary_file', { path: txt });
  await bulk(main).getByRole('button', { name: 'Export…' }).click();
  await main.getByRole('menuitem', { name: 'As text…' }).click();
  await expect(toastWith(main, 'Exported 2 entries')).toContainText('Snippets need CSV: 1 left out.');
  expect(fs.readFileSync(txt, 'utf8')).toBe(
    '# Yap dictionary: what Yap hears → what it writes, one a line.\r\npower to keep → Parakeet\r\njaison → JSON\r\n'
  );
});

test('2,000 entries stay quick: the view opens, a search filters, typing in a row, Ctrl+A, more rows as you scroll, a bulk delete undone', async ({
  yap,
  main,
  shot,
}) => {
  const big = Array.from({ length: 2000 }, (_, i) => ({
    from: `word ${i} heard`,
    to: `Word${String(i).padStart(4, '0')}`,
    fuzzy: true,
    snippet: false,
  }));
  big.push({ from: 'my sign-off', to: 'Speak soon,\nRobin', fuzzy: true, snippet: true });
  await main.setViewportSize({ width: 1200, height: 800 });
  await setConfig(yap, main, { dictionary: big });
  await main.evaluate(() => localStorage.removeItem('yap.dictionarySort'));

  const opened = Date.now();
  await openView(main, 'Dictionary');
  const rows = corrections(main).locator('.dict-row');
  await expect(rows).toHaveCount(200); // drawn 200 at a time
  const openMs = Date.now() - opened;
  await expect(corrections(main).locator('.sec-count')).toHaveText('2,000');
  await shot(main, '07-two-thousand');

  /** Type `value` into the box `selector` finds, timed to the frame after the page caught up. */
  const timed = (selector, value) =>
    main.evaluate(
      async ([sel, text]) => {
        const box = document.querySelector(sel);
        const t = performance.now();
        box.value = text;
        box.dispatchEvent(new Event('input', { bubbles: true }));
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        return { ms: performance.now() - t, rows: document.querySelectorAll('.dict-row').length };
      },
      [selector, value]
    );
  const SEARCH = 'input[aria-label="Search the dictionary"]';
  const search = await timed(SEARCH, 'word1999');
  expect(search.rows).toBe(1);
  const cleared = await timed(SEARCH, '');
  expect(cleared.rows).toBe(200);
  const typing = await timed('.dict-row input[aria-label="Heard"]', 'word zero heard');
  console.log(`2,000 entries: open ${openMs} ms, search ${search.ms.toFixed(1)} ms, clear ${cleared.ms.toFixed(1)} ms, a keystroke ${typing.ms.toFixed(1)} ms`);
  // Generous for CI's slower runners; on a desktop they're a fraction of these.
  expect(openMs).toBeLessThan(5_000);
  expect(search.ms).toBeLessThan(750);
  expect(cleared.ms).toBeLessThan(1_500);
  expect(typing.ms).toBeLessThan(250);
  await expectStore(yap, 'config.json', (c) => c.dictionary[0].from === 'word zero heard');

  // Scrolling to the end of the drawn rows draws the next 200.
  await corrections(main).getByRole('button', { name: /^Show 200 more \(1,800 not shown yet\)/ }).scrollIntoViewIfNeeded();
  await expect(rows).toHaveCount(400);

  // Ctrl+A: all 2,001, drawn or not.
  await check(main, 'Word0000').focus();
  await main.keyboard.press('Control+a');
  await expect(bulk(main)).toContainText('2,001 entries selected');
  await bulk(main).getByRole('button', { name: 'Delete…' }).click();
  const ask = main.getByRole('alertdialog', { name: 'Delete 2,001 entries?' });
  await expect(ask).toContainText('2,000 corrections and 1 snippet go from your dictionary.');
  await ask.getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionary.length === 0);
  await expect(corrections(main).getByText('No corrections yet.')).toBeVisible();
  await toastWith(main, 'Deleted 2,001 entries').getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'config.json', (c) => c.dictionary.length === 2001 && c.dictionary[1999].to === 'Word1999');
  await expect(rows).toHaveCount(400); // as many as were drawn
  await setConfig(yap, main, { dictionary: DICT });
});

test('in the dark theme: the tools, a star, a selection, a search and the import preview', async ({ yap, main, shot }) => {
  await freshView(yap, main, DICT.map((e) => (e.to === 'Vexwell' ? { ...e, starred: true } : e)));
  const pick = async (label) => {
    const dialog = await openSettings(main, 'General');
    await dialog
      .getByRole('group', { name: 'Appearance' })
      .getByRole('tablist', { name: 'Theme' })
      .getByRole('tab', { name: label, exact: true })
      .click();
    await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase());
    await closeSettings(main);
  };
  await pick('Dark');
  try {
    await check(main, 'Kubernetes').click();
    await check(main, 'JSON').click();
    await shot(main, '08-dark-selected');
    await searchBox(main).fill('e');
    await sortBox(main).selectOption('used');
    await shot(main, '09-dark-search-most-used');
    const file = path.join(yap.runDir, 'dark.csv');
    fs.writeFileSync(file, 'heard,written\njaison,Json\norbit lee,Orbitly\nZentrix,Zentrix\n');
    await yap.invoke('e2e_dictionary_file', { path: file });
    await main.getByRole('button', { name: 'Import…' }).click();
    const dialog = main.getByRole('dialog', { name: 'Import from “dark.csv”' });
    await expect(dialog).toBeVisible();
    await shot(main, '10-dark-import-preview');
    await main.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
  } finally {
    await pick('Light');
  }
});
