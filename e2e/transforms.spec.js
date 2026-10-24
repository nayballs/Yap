// Saved transforms (transforms.rs, 2026-10-24): Settings → Commands →
// Transforms — Yap's five in order, Translate's language, adding, editing,
// moving and deleting your own, a shortcut recorded and checked against the
// other shortcuts (and they against it), the slot it takes in the input
// hook, Settings → Shortcuts' link, the search — and what a transform does,
// through the test hooks: the selection (`e2e_transform`'s stand-in for
// text selected), nothing selected right after a dictation (the last
// dictation, by the rewrite shortcut's rules and its switch), nothing at
// all, a terminal (never), the AI's answer the same as the text, the AI
// failing, and the bar's "Polish ✓ · Undo".
//
// A test run reads and pastes into no other app: the transform and the
// dictations go to a stand-in window (`e2e_target_window`), and the AI is a
// stand-in (support/fake-llm.js). The real windows are the robot's
// (e2e-real/transforms.real.js).
import fs from 'node:fs';
import { test as base, expect, openSettings, closeSettings, settingsDialog, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript, transformText, transformInstruction } from './support/fake-llm.js';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'transforms' });
    },
    { scope: 'worker' },
  ],
});

const SAID = 'hey can you send me the report by friday';
const CLEANED = 'Hey, can you send me the report by Friday?';
const SELECTED = 'so basically i think we should like ship it on friday ';
const POLISHED = 'I think we should ship it on Friday.';
const SPANISH = 'Hola, ¿puedes enviarme el informe el viernes?';
const FRIENDLIER = 'Hi! Could you send me the report by Friday? Thanks so much!';

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const names = (cfg) => cfg.transforms.map((t) => t.id);

/** A key as Chromium sends it (the shortcut picker reads keyCode). */
async function pressKey(page, key, vk) {
  const cdp = await page.context().newCDPSession(page);
  try {
    const ev = { key, code: key, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
    await cdp.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...ev });
    await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', ...ev });
  } finally {
    await cdp.detach().catch(() => {});
  }
}

test('Settings → Commands → Transforms: Yap\'s five, Translate\'s language, your own added, edited, moved and deleted, a shortcut recorded', async ({
  yap,
  main,
  shot,
}) => {
  expect(names(await yap.invoke('get_config'))).toEqual([
    'polish',
    'shorter',
    'grammar',
    'translate',
    'prompt',
  ]);
  const dialog = await openSettings(main, 'Commands');
  const group = dialog.getByRole('group', { name: 'Transforms' });
  await group.scrollIntoViewIfNeeded();
  const rows = group.locator('[data-transform]');
  await expect(rows).toHaveCount(5);
  await expect(rows.locator('.name')).toHaveText(['Polish', 'Shorter', 'Fix grammar only', 'Translate to Spanish', 'Make it a prompt']);
  // Yap's own: no Edit or Delete, their instruction is Yap's.
  await expect(group.getByRole('button', { name: /^(Edit|Delete) / })).toHaveCount(0);
  await shot(main, '01-transforms-built-ins');

  // Translate's language.
  const translate = group.locator('[data-transform="translate"]');
  await translate.getByRole('combobox', { name: 'Translate to' }).selectOption('French');
  await expect(translate.locator('.name')).toHaveText('Translate to French');
  await expectStore(yap, 'config.json', (c) => c.transforms.find((t) => t.id === 'translate')?.language === 'French');

  // A shortcut for Polish: the dictation hotkey (F24 in the suite) refused,
  // F22 taken, and its slot in the input hook set.
  const polish = group.locator('[data-transform="polish"]');
  await polish.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F24', 135);
  await expect(polish).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'F22', 133);
  await expectStore(yap, 'config.json', (c) => c.transforms.find((t) => t.id === 'polish')?.hotkey === 'kb:133');
  await expect(polish.getByRole('button', { name: /^F22\s*Change$/ })).toBeVisible();
  await expect.poll(() => yap.invoke('e2e_transform_slots')).toEqual([{ id: 'polish', key: 'keyboard vkey 133' }]);
  await shot(main, '02-transforms-polish-shortcut');

  // Your own: added, then on a key of its own — not Polish's.
  await group.getByRole('button', { name: 'Add a transform' }).click();
  const form = group.getByRole('group', { name: 'New transform' });
  await expect(form.getByRole('textbox', { name: 'Transform name' })).toBeFocused();
  await expect(form.getByRole('button', { name: 'Add transform' })).toBeDisabled();
  await form.getByRole('textbox', { name: 'Transform name' }).fill('Friendlier');
  await form.getByRole('textbox', { name: 'What the transform does' }).fill('Make it warmer and friendlier.');
  await shot(main, '03-transforms-add');
  await form.getByRole('button', { name: 'Add transform' }).click();
  await expectStore(yap, 'config.json', (c) => c.transforms.length === 6 && c.transforms[5].name === 'Friendlier' && c.transforms[5].builtin === '');
  const mine = group.locator('[data-transform]').nth(5);
  await expect(mine.locator('.name')).toHaveText('Friendlier');
  await expect(mine.locator('.line')).toHaveText('Make it warmer and friendlier.');
  await mine.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F22', 133);
  await expect(mine).toContainText('Already used by the Polish transform');
  await pressKey(main, 'F21', 132);
  await expectStore(yap, 'config.json', (c) => c.transforms[5].hotkey === 'kb:132');
  const id = (await yap.invoke('get_config')).transforms[5].id;
  await expect.poll(() => yap.invoke('e2e_transform_slots')).toEqual([
    { id: 'polish', key: 'keyboard vkey 133' },
    { id, key: 'keyboard vkey 132' },
  ]);

  // Moved up: the order is the config's (and the slots').
  await mine.getByRole('button', { name: 'Move Friendlier up' }).click();
  await expectStore(yap, 'config.json', (c) => names(c).join() === `polish,shorter,grammar,translate,${id},prompt`);
  await expect(group.getByRole('button', { name: 'Move Polish up' })).toBeDisabled();

  // Edited in place: Esc leaves it as it was, Save keeps the change.
  const edited = group.locator(`[data-transform="${id}"]`);
  await edited.getByRole('button', { name: 'Edit Friendlier' }).click();
  const editForm = group.getByRole('group', { name: 'Edit transform' });
  await editForm.getByRole('textbox', { name: 'What the transform does' }).fill('Make it warmer, friendlier and short.');
  await editForm.getByRole('button', { name: 'Save' }).click();
  await expectStore(yap, 'config.json', (c) => c.transforms.find((t) => t.id === id)?.instruction === 'Make it warmer, friendlier and short.');
  await shot(main, '04-transforms-your-own');

  // Settings → Shortcuts: a fixed shortcut can't take a transform's key,
  // and its link row goes to the list.
  const shortcuts = await openSettings(main, 'Shortcuts');
  const asSaid = shortcuts.locator('[data-setting="shortcut-as-said"]');
  await asSaid.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'F22', 133);
  await expect(asSaid).toContainText('Already used by the Polish transform');
  await pressKey(main, 'F20', 131);
  await expectStore(yap, 'config.json', (c) => c.asSaidHotkey === 'kb:131');
  await asSaid.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.asSaidHotkey === '');
  const link = shortcuts.locator('[data-setting="shortcut-transforms"]');
  await expect(link).toContainText('2 shortcuts');
  await shot(main, '05-shortcuts-transforms-link');
  await link.click();
  await expect(settingsDialog(main).getByRole('group', { name: 'Transforms' })).toBeInViewport();

  // Deleted, behind the confirm dialog: its shortcut goes with it.
  const again = settingsDialog(main).locator(`[data-transform="${id}"]`);
  await again.getByRole('button', { name: 'Delete Friendlier' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toContainText('Delete “Friendlier”?');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'config.json', (c) => !c.transforms.some((t) => t.id === id));
  await expect.poll(() => yap.invoke('e2e_transform_slots')).toEqual([{ id: 'polish', key: 'keyboard vkey 133' }]);

  // Polish's shortcut removed.
  await settingsDialog(main).locator('[data-transform="polish"]').getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.transforms.every((t) => !t.hotkey));
  await expect.poll(() => yap.invoke('e2e_transform_slots')).toEqual([]);

  // Settings search finds it.
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await search.fill('translate');
  const found = settingsDialog(main).getByRole('listbox', { name: 'Settings found' });
  await expect(found.getByRole('option').filter({ hasText: 'Transforms' }).first()).toBeVisible();
  await search.fill('');
  await closeSettings(main);
});

test('a transform on the selection, on the last dictation, on nothing, in a terminal; unchanged; failed; and the bar\'s Undo', async ({ yap, main, shot }) => {
  const llm = await startFakeLlm();
  llm.reply('cleanup', (user) => (cleanupTranscript(user) === SAID ? CLEANED : cleanupTranscript(user)));
  llm.reply('transform', (user) => {
    const asked = transformInstruction(user);
    const text = transformText(user);
    if (/^Polish/.test(asked)) return text === POLISHED ? POLISHED : `Here is the polished text:\n${POLISHED}`;
    if (/into French/.test(asked)) return SPANISH;
    if (/friendlier/i.test(asked)) return FRIENDLIER;
    return text;
  });
  const asked = () => llm.requests.filter((r) => r.kind === 'transform');
  const lastAsked = () => {
    const r = asked().at(-1);
    return r && { text: transformText(r.user), instruction: transformInstruction(r.user), system: r.system };
  };
  await setConfig(yap, main, { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: llm.base, ppModel: 'fake-cleanup', ppApiKey: '' });
  await yap.invoke('e2e_target_window', { on: true });
  const capsule = yap.overlay.locator('.capsule');
  try {
    // The selection: the AI gets it (trimmed) under Yap's guardrails and
    // Polish's own instruction; its preamble comes off; the paste would go
    // over the selection (a test run pastes nothing).
    let before = log(yap).length;
    let r = await yap.invoke('e2e_transform', { id: 'polish', selection: SELECTED });
    expect(r).toMatchObject({ name: 'Polish', source: 'selection', outcome: 'testMode', text: POLISHED });
    expect(lastAsked().text).toBe(SELECTED.trim());
    expect(lastAsked().instruction).toMatch(/^Polish the text/);
    expect(lastAsked().system).toMatch(/^You transform text/);
    expect(lastAsked().system).toContain('never answer it');
    expect(log(yap).slice(before)).toContain('e2e: not pasting the text');
    // The bar: "Polish ✓ · Undo"; Undo would put the text back.
    await expect(capsule).toContainText('Polish');
    const undo = capsule.getByRole('button', { name: 'Undo' });
    await expect(undo).toBeVisible();
    await yap.overlay.waitForTimeout(250);
    await shot(yap.overlay, '06-bar-polish-undo');
    before = log(yap).length;
    await undo.click();
    await expect.poll(() => log(yap).slice(before)).toContain('e2e: not undoing the transform');
    await expect(capsule).toBeHidden();

    // Nothing selected, right after a dictation into that window: the
    // dictation is what changes (Translate to French, set in the test before).
    await yap.invoke('e2e_dictation', { text: SAID });
    before = log(yap).length;
    r = await yap.invoke('e2e_transform', { id: 'translate' });
    expect(r).toMatchObject({ name: 'Translate to French', source: 'last', outcome: 'testMode', text: SPANISH });
    expect(lastAsked().text).toBe(CLEANED);
    expect(lastAsked().instruction).toContain('into French');
    expect(log(yap).slice(before)).toContain('e2e: not rewriting the last dictation');

    // "Rewrite your last dictation" off: nothing to transform, said, the AI
    // not asked.
    await setConfig(yap, main, { rewriteLastDictation: false });
    await yap.invoke('e2e_dictation', { text: SAID });
    let count = asked().length;
    r = await yap.invoke('e2e_transform', { id: 'polish' });
    expect(r).toMatchObject({ source: '', outcome: 'nothing' });
    expect(asked().length).toBe(count);
    await expect(main.getByRole('status').filter({ hasText: 'Nothing to transform' })).toBeVisible();
    await shot(main, '07-nothing-to-transform');
    await setConfig(yap, main, { rewriteLastDictation: true });

    // A terminal: never, even with text selected.
    count = asked().length;
    r = await yap.invoke('e2e_transform', { id: 'polish', selection: SELECTED, terminal: true });
    expect(r.outcome).toBe('terminal');
    expect(asked().length).toBe(count);
    await expect(main.getByRole('status').filter({ hasText: "Transforms don't work in a terminal" })).toBeVisible();

    // The AI gave back the same text: nothing pasted, "no changes needed".
    before = log(yap).length;
    r = await yap.invoke('e2e_transform', { id: 'polish', selection: POLISHED });
    expect(r.outcome).toBe('unchanged');
    expect(log(yap).slice(before)).not.toContain('e2e: not pasting the text');
    await expect(capsule).toContainText('Polish: no changes needed');
    await expect(capsule).toBeHidden({ timeout: 6_000 });

    // The person's own transform, with its own instruction.
    const cfg = await yap.invoke('get_config');
    const mine = { id: 't-e2e', builtin: '', name: 'Friendlier', instruction: 'Make it warmer and friendlier.', hotkey: '', language: '' };
    await setConfig(yap, main, { transforms: [...cfg.transforms, mine] });
    r = await yap.invoke('e2e_transform', { id: 't-e2e', selection: 'send me the report by friday' });
    expect(r).toMatchObject({ name: 'Friendlier', outcome: 'testMode', text: FRIENDLIER });
    expect(lastAsked().instruction).toBe('Make it warmer and friendlier.');

    // The AI failing: said on the bar, nothing pasted.
    llm.failNext('transform', 1);
    before = log(yap).length;
    r = await yap.invoke('e2e_transform', { id: 'polish', selection: SELECTED });
    expect(r.outcome).toBe('failed');
    expect(log(yap).slice(before)).not.toContain('e2e: not pasting the text');
    await expect(capsule).toContainText('Polish failed via custom');
  } finally {
    await yap.invoke('e2e_target_window', { on: false });
    const cfg = await yap.invoke('get_config');
    await setConfig(yap, main, {
      postProcessEnabled: false,
      ppBaseUrl: '',
      rewriteLastDictation: true,
      transforms: cfg.transforms.filter((t) => t.id !== 't-e2e'),
    });
    await llm.close();
  }
});
