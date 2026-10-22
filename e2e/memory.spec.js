// Memory (src-tauri/src/memory.rs, Settings → Memory): what Yap remembers
// about the person. The model is the scripted fake (support/fake-llm.js),
// which knows memory's passes by their prompt ("You keep Yap's memory of
// the user") and answers them with the changes a spec gives it, so this
// checks Yap's side: the switches save, an item edited by hand lands in
// memory.json (a secret refused), "Tell Yap what to change" shows its diff
// and applies with Undo, an import is reviewed change by change, the export
// is a real .zip, Clear all asks first, and a chat that went quiet brings
// the "Yap remembered…" toast with Undo and Manage. About you's name,
// region and note show under You (each row goes to About you), and what
// they say isn't kept a second time from an import. What a real model
// learns is the model evaluations' job (docs/model-evals.md).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';
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
        name: 'memory',
        // Windows' region as a test run has it (about_you.rs): the UK.
        env: {
          YAP_E2E_LOCALE: JSON.stringify({ locale: 'en-GB', country: 'GB', currency: 'GBP', symbol: '£', metric: true, timeZone: 'GMT Standard Time' }),
        },
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

/** The fake model's next memory answer: these changes when the pass's input holds `when`. */
const answerMemory = (fakeLlm, when, changes) =>
  fakeLlm.reply('memory', (user) => (user.includes(when) ? JSON.stringify({ changes }) : '{"changes": []}'));

const items = (yap) => yap.readJson('memory.json')?.items ?? [];
const toastWith = (main, text) => main.getByRole('status').filter({ hasText: text });

test('Settings → Memory: the switches, and an empty memory', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Memory');
  await expect(dialog.getByRole('heading', { level: 1, name: 'Memory' })).toBeVisible();
  const sw = (name) => dialog.getByRole('button', { name, exact: true });
  await expect(sw('Search and reference past chats')).toHaveAttribute('aria-pressed', 'true');
  await expect(sw('Learn from my chats')).toHaveAttribute('aria-pressed', 'true');
  await expect(sw('Learn from meetings')).toHaveAttribute('aria-pressed', 'false');
  await expect(sw('Include sensitive topics in memory')).toHaveAttribute('aria-pressed', 'false');
  for (const section of ['You', 'People', 'Projects', 'Topics']) {
    await expect(dialog.getByRole('group', { name: section, exact: true })).toBeVisible();
  }
  await expect(dialog.getByRole('group', { name: 'People', exact: true })).toContainText('Nothing yet.');
  await expect(dialog.getByText('never sent to Yap')).toBeVisible();
  // An AI model is set up (the fake): no "needs an AI model" card.
  await expect(dialog.getByText('Yap needs an AI model for memory')).toHaveCount(0);
  await shot(main, '01-empty');

  // The switches save (Settings' auto-save) and stay.
  await sw('Learn from meetings').click();
  await expectStore(yap, 'config.json', (c) => c.memoryLearnMeetings === true);
  await sw('Include sensitive topics in memory').click();
  await expectStore(yap, 'config.json', (c) => c.memorySensitive === true);
  await sw('Learn from meetings').click();
  await sw('Include sensitive topics in memory').click();
  await expectStore(yap, 'config.json', (c) => c.memoryLearnMeetings === false && c.memorySensitive === false);
  await closeSettings(main);
});

test('an item edited by hand: added, changed, the dictation switch, a secret refused, deleted and undone', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Memory');
  const people = dialog.getByRole('group', { name: 'People', exact: true });
  await people.getByRole('button', { name: 'Add a person' }).click();
  const add = people.getByRole('group', { name: 'Add a person' });
  await add.getByRole('textbox', { name: 'Name' }).fill('Rochelle');
  await add.getByRole('textbox', { name: 'Who they are to you' }).fill('Sister');
  await add.getByRole('textbox', { name: 'Facts, one per line' }).fill('Lives in York\nBirthday 3 May');
  await add.getByRole('button', { name: 'Save' }).click();
  await expectStore(yap, 'memory.json', (m) =>
    m.items.some((i) => i.section === 'people' && i.title === 'Rochelle' && i.summary === 'Sister' && i.details.join('|') === 'Lives in York|Birthday 3 May' && i.source.kind === 'manual')
  );
  const row = people.getByRole('button', { name: /^Rochelle/ });
  await expect(row).toContainText('Sister');
  await expect(row).toContainText('Updated');

  // Profile lines go into the one Profile item.
  const you = dialog.getByRole('group', { name: 'You', exact: true });
  await you.getByRole('button', { name: /^Profile/ }).click();
  await you.getByRole('textbox', { name: 'Facts, one per line' }).fill('Works night shifts as a nurse\nLives in Norwich');
  await you.getByRole('button', { name: 'Save' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.some((i) => i.section === 'profile' && i.details.length === 2));
  await expect(you.getByRole('button', { name: /^Profile/ })).toContainText('Works night shifts as a nurse · Lives in Norwich');
  await shot(main, '02-edited');

  // Open Rochelle: "Spell this name in dictation" (on; her name is fixed in dictation).
  await row.click();
  const edit = people.getByRole('group', { name: 'Edit Rochelle' });
  const spell = edit.getByRole('button', { name: 'Spell this name in dictation' });
  await expect(spell).toHaveAttribute('aria-pressed', 'true');
  // What it does for her name (near-misses fixed where Windows' spell
  // checker can tell names from words, else only the Whisper prompt).
  await expect(edit).toContainText(/fixes near-misses|tells Whisper models/);
  await spell.click();
  await expectStore(yap, 'memory.json', (m) => m.items.find((i) => i.title === 'Rochelle')?.dictation === false);

  // A secret is never kept, even typed by hand.
  await edit.getByRole('textbox', { name: 'Facts, one per line' }).fill('Lives in York\nPhone 07700 900123');
  await edit.getByRole('button', { name: 'Save' }).click();
  await expect(toastWith(main, "Couldn't save that")).toContainText('a phone number');
  expect(items(yap).find((i) => i.title === 'Rochelle').details).toEqual(['Lives in York', 'Birthday 3 May']);

  // Delete, then Undo on the toast.
  await edit.getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'memory.json', (m) => !m.items.some((i) => i.title === 'Rochelle'));
  const deleted = toastWith(main, 'Deleted “Rochelle”');
  await expect(deleted).toBeVisible();
  await deleted.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.some((i) => i.title === 'Rochelle'));
  await expect(people.getByRole('button', { name: /^Rochelle/ })).toBeVisible();
  await closeSettings(main);
});

test('"Tell Yap what to change": the model\'s changes as a diff, applied, and undone', async ({ yap, main, fakeLlm, shot }) => {
  answerMemory(fakeLlm, "My dog's name is Beans", [
    { action: 'add', section: 'profile', facts: ['Has a dog called Beans'], say: 'your dog is called Beans' },
  ]);
  const dialog = await openSettings(main, 'Memory');
  await dialog.getByRole('textbox', { name: 'Tell Yap what to change' }).fill("My dog's name is Beans");
  await dialog.getByRole('button', { name: 'Suggest changes' }).click();
  const diff = dialog.getByRole('list', { name: 'Proposed changes' });
  await expect(diff).toContainText('Has a dog called Beans');
  await expect(diff).toContainText('Profile');
  // Nothing changes until it's applied.
  expect(items(yap).some((i) => i.details?.includes('Has a dog called Beans'))).toBe(false);
  await diff.scrollIntoViewIfNeeded();
  await shot(main, '03-tell-diff');

  // What the model was asked: memory's own prompt, memory with ids, the words.
  const asked = fakeLlm.requests.filter((r) => r.kind === 'memory').at(-1);
  expect(asked.system.startsWith("You keep Yap's memory of the user")).toBe(true);
  expect(asked.user).toContain('What Yap remembers now:\n[');
  expect(asked.user).toContain("What the user wants changed:\nMy dog's name is Beans");

  await dialog.getByRole('button', { name: 'Apply' }).click();
  await expectStore(yap, 'memory.json', (m) =>
    m.items.some((i) => i.section === 'profile' && i.details.includes('Has a dog called Beans') && i.source.kind === 'tell')
  );
  await expect(dialog.getByRole('textbox', { name: 'Tell Yap what to change' })).toHaveValue('');
  const updated = toastWith(main, 'Memory updated');
  await expect(updated).toContainText('your dog is called Beans');
  await updated.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'memory.json', (m) => !m.items.some((i) => i.details?.includes('Has a dog called Beans')));

  // Nothing to change: said so, nothing applied.
  answerMemory(fakeLlm, 'never matches', []);
  await dialog.getByRole('textbox', { name: 'Tell Yap what to change' }).fill('What time is it?');
  await dialog.getByRole('button', { name: 'Suggest changes' }).click();
  await expect(dialog.getByRole('status').filter({ hasText: 'Nothing in memory changes for that.' })).toBeVisible();
  await closeSettings(main);
});

test('an import: the prompt to copy, the answer pasted, each change reviewed', async ({ yap, main, fakeLlm, shot }) => {
  answerMemory(fakeLlm, 'What the other assistant remembers about the user', [
    { action: 'add', section: 'preferences', facts: ['Prefers concise answers'] },
    { action: 'add', section: 'people', name: 'Priya', about: 'Colleague at Acme', facts: ['Designer'] },
    { action: 'add', section: 'topics', name: "Baldur's Gate 3", about: 'A game they play' },
  ]);
  const dialog = await openSettings(main, 'Memory');
  await dialog.getByRole('button', { name: 'Import', exact: true }).click();
  await expect(dialog.getByText('Please list everything you remember about me')).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Copy prompt' })).toBeVisible();
  await dialog
    .getByRole('textbox', { name: "The other assistant's answer" })
    .fill("```\n- Prefers concise answers\n- Colleague Priya at Acme, a designer\n- Plays Baldur's Gate 3\n```");
  await dialog.getByRole('button', { name: 'Turn into memory' }).click();
  const review = dialog.getByRole('list', { name: 'Proposed changes' });
  await expect(review.getByRole('checkbox')).toHaveCount(3);
  await review.getByRole('checkbox', { name: "Keep Baldur's Gate 3" }).uncheck();
  await review.scrollIntoViewIfNeeded();
  await shot(main, '04-import-review');
  await dialog.getByRole('button', { name: 'Save 2' }).click();
  await expectStore(yap, 'memory.json', (m) =>
    m.items.some((i) => i.title === 'Priya' && i.source.kind === 'import') &&
    m.items.some((i) => i.section === 'preferences' && i.details.includes('Prefers concise answers')) &&
    !m.items.some((i) => i.title === "Baldur's Gate 3")
  );
  await expect(toastWith(main, 'Memory updated: 2 changes')).toBeVisible();
  await closeSettings(main);
});

test('a quiet chat is learned from: "Yap remembered…", Undo, and Manage', async ({ yap, main, fakeLlm, shot }) => {
  await closeSettings(main);
  answerMemory(fakeLlm, 'My sister Siobhan', [
    { action: 'add', section: 'people', name: 'Siobhan', about: 'Sister', facts: ['Lives in York'], say: 'your sister is Siobhan' },
  ]);
  fakeLlm.scriptChat([{ content: 'Lovely! The Jorvik Viking Centre is great on a rainy day.' }]);
  const sent = await yap.invoke('chat_send', { conversationId: null, text: 'My sister Siobhan is coming to stay this weekend, ideas for York?' });
  // What a quiet chat gets after 5 minutes (the scheduler doesn't run in a test).
  const learned = await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  expect(learned.done.map((d) => d.title)).toEqual(['Siobhan']);
  // It read the chat: the person's words in full.
  const asked = fakeLlm.requests.filter((r) => r.kind === 'memory').at(-1);
  expect(asked.user).toContain('User: My sister Siobhan is coming to stay this weekend, ideas for York?');
  expect(asked.system).toContain('Never keep sensitive topics');
  const t = toastWith(main, 'Yap remembered: your sister is Siobhan');
  await expect(t).toBeVisible();
  await expect(t.getByRole('button', { name: 'Manage' })).toBeVisible();
  await shot(main, '05-remembered-toast');
  await t.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'memory.json', (m) => !m.items.some((i) => i.title === 'Siobhan'));

  // Read once: nothing new in the chat, no second pass (and no model call).
  const before = fakeLlm.requests.filter((r) => r.kind === 'memory').length;
  await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  expect(fakeLlm.requests.filter((r) => r.kind === 'memory').length).toBe(before);

  // The chat goes on: learned again, and Manage opens Settings → Memory on it.
  answerMemory(fakeLlm, 'Siobhan loves hiking', [
    { action: 'add', section: 'people', name: 'Siobhan', about: 'Sister', facts: ['Loves hiking'], say: 'Siobhan loves hiking' },
  ]);
  fakeLlm.scriptChat([{ content: 'Then try the Dales Way!' }]);
  await yap.invoke('chat_send', { conversationId: sent.conversationId, text: 'Oh and Siobhan loves hiking, I should plan a walk' });
  await yap.invoke('e2e_memory_learn', { conversationId: sent.conversationId });
  const again = toastWith(main, 'Yap remembered: Siobhan loves hiking');
  await expect(again).toBeVisible();
  await again.getByRole('button', { name: 'Manage' }).click();
  const dialog = main.getByRole('dialog', { name: 'Settings' });
  await expect(dialog.getByRole('heading', { level: 1, name: 'Memory' })).toBeVisible();
  const row = dialog.getByRole('group', { name: 'People', exact: true }).getByRole('button', { name: /^Siobhan/ });
  await expect(row).toContainText('Sister');
  await row.click();
  await expect(dialog.getByText('Learned from a chat')).toBeVisible();
  await shot(main, '06-learned-in-settings');
  await closeSettings(main);
});

test('export: a .zip of Markdown, one file per section', async ({ yap, main }) => {
  const dialog = await openSettings(main, 'Memory');
  // The button opens Windows' save dialog, which a test can't click: the
  // export it then writes (`memory_export`, as the button calls it).
  await expect(dialog.getByRole('button', { name: 'Export', exact: true })).toBeVisible();
  await yap.invoke('memory_save_item', { item: { section: 'people', title: 'Aoife', summary: 'Cousin', details: ['Lives in Galway'] } });
  const file = path.join(yap.runDir, 'memory-export.zip');
  await yap.invoke('memory_export', { path: file });
  const zip = fs.readFileSync(file);
  expect(zip.subarray(0, 4)).toEqual(Buffer.from([0x50, 0x4b, 0x03, 0x04]));
  for (const name of ['You.md', 'People.md', 'Projects.md', 'Topics.md']) expect(zip.includes(name)).toBe(true);
  // People.md (the second file, Deflate): its text inflates back.
  const zlib = await import('node:zlib');
  let at = 0;
  const files = {};
  while (zip.readUInt32LE(at) === 0x04034b50) {
    const packed = zip.readUInt32LE(at + 18);
    const nameLen = zip.readUInt16LE(at + 26);
    const name = zip.subarray(at + 30, at + 30 + nameLen).toString();
    const start = at + 30 + nameLen;
    files[name] = zlib.inflateRawSync(zip.subarray(start, start + packed)).toString();
    at = start + packed;
  }
  expect(Object.keys(files)).toEqual(['You.md', 'People.md', 'Projects.md', 'Topics.md']);
  expect(files['People.md']).toContain('## Aoife\n\nCousin\n\n- Lives in Galway');
  await closeSettings(main);
});

test('Clear all memory asks first, then clears (with Undo)', async ({ yap, main, shot }) => {
  await yap.invoke('memory_save_item', { item: { section: 'projects', title: 'Allotment', summary: 'Vegetable plot', details: [] } });
  const dialog = await openSettings(main, 'Memory');
  await expect(dialog.getByRole('group', { name: 'Projects', exact: true }).getByRole('button', { name: /^Allotment/ })).toBeVisible();
  await dialog.getByRole('button', { name: 'Clear all', exact: true }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toContainText('Clear all memory?');
  await shot(main, '07-clear-confirm');
  await confirm.getByRole('button', { name: 'Clear all' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.length === 0);
  await expect(dialog.getByRole('group', { name: 'Projects', exact: true })).toContainText('Nothing yet.');
  const cleared = toastWith(main, 'Memory cleared');
  await cleared.getByRole('button', { name: 'Undo' }).click();
  await expectStore(yap, 'memory.json', (m) => m.items.some((i) => i.title === 'Allotment'));
  await closeSettings(main);
});

test("You: About you's name, region and note, each a way to About you, and not kept twice", async ({ yap, main, fakeLlm, shot }) => {
  const dialog = await openSettings(main, 'Memory');
  const you = dialog.getByRole('group', { name: 'You', exact: true });
  const row = (key) => you.locator(`[data-about="${key}"]`);
  // Nothing set: the name and note say so; the region is Windows' (the UK).
  await expect(row('name')).toContainText('Not set: what Yap should call you');
  await expect(row('region')).toContainText('United Kingdom (from Windows) · Prices in £ · miles and °C · British spelling · UK time');
  await expect(row('note')).toContainText('Not set: anything the AI should know about you');
  await expect(row('name')).toContainText('In About you');

  // A row goes to About you, where they're set.
  await row('name').click();
  const aboutYou = dialog.locator('#settings-about-you');
  await expect(aboutYou).toBeVisible();
  await aboutYou.getByRole('textbox', { name: 'What should Yap call you?' }).fill('Declan');
  await aboutYou.getByRole('textbox', { name: 'Anything the AI should know about you' }).fill('I work weekends at a bakery in Norwich.');
  await expectStore(yap, 'config.json', (c) => c.aboutName === 'Declan' && c.aboutMe?.startsWith('I work weekends'));

  // Back in Memory: the rows follow.
  await openSettings(main, 'Memory');
  await expect(row('name')).toContainText('Declan');
  await expect(row('note')).toContainText('“I work weekends at a bakery in Norwich.”');
  await you.scrollIntoViewIfNeeded();
  await shot(main, '08-about-you');

  // What About you says isn't kept twice: an import (as a learning pass)
  // repeating the note and the name keeps only what's new.
  answerMemory(fakeLlm, 'learning Spanish', [
    { action: 'add', section: 'profile', facts: ['Works weekends at a bakery in Norwich', 'Learning Spanish for a trip to Seville'] },
    { action: 'add', section: 'profile', facts: ['Name is Declan'] },
  ]);
  await dialog.getByRole('button', { name: 'Import', exact: true }).click();
  await dialog
    .getByRole('textbox', { name: "The other assistant's answer" })
    .fill('- Name is Declan\n- Works weekends at a bakery in Norwich\n- Is learning Spanish for a trip to Seville');
  await dialog.getByRole('button', { name: 'Turn into memory' }).click();
  const review = dialog.getByRole('list', { name: 'Proposed changes' });
  await expect(review.getByRole('checkbox')).toHaveCount(1);
  await expect(review).toContainText('Learning Spanish for a trip to Seville');
  await expect(review).not.toContainText('bakery');
  await expect(review).not.toContainText('Name is Declan');
  await dialog.getByRole('button', { name: 'Discard' }).click();
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();

  // Export: About you heads You.md (the first file in the .zip).
  const file = path.join(yap.runDir, 'memory-export-about.zip');
  await yap.invoke('memory_export', { path: file });
  const zip = fs.readFileSync(file);
  const zlib = await import('node:zlib');
  const packed = zip.readUInt32LE(18);
  const nameLen = zip.readUInt16LE(26);
  const youMd = zlib.inflateRawSync(zip.subarray(30 + nameLen, 30 + nameLen + packed)).toString();
  expect(youMd).toContain('## About you\n\n- Name: Declan\n- Region: United Kingdom\n- What you told Yap: “I work weekends at a bakery in Norwich.”');

  // Left as they were for the other tests.
  await row('name').click();
  await aboutYou.getByRole('textbox', { name: 'What should Yap call you?' }).fill('');
  await aboutYou.getByRole('textbox', { name: 'Anything the AI should know about you' }).fill('');
  await expectStore(yap, 'config.json', (c) => !c.aboutName && !c.aboutMe);
  await closeSettings(main);
});
