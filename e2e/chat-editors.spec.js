// Chat apps and code editors (2026-10-29): stacked messages ("new message"
// said: several messages, each sent — pasting/stacked.rs), @mentions ("at
// Rochelle" in Slack typed and picked from its list — pasting/mentions.rs),
// file names and code names said aloud ("user service dot ts" →
// userService.ts — dictionary/screen_terms/code.rs), file tags in AI chat
// boxes and coding CLIs (pasting/file_tags.rs), all pasted in steps
// (pasting/compose.rs), and the Send shortcut (pasting/send_key.rs).
//
// A test run reads no other app's window and pastes, types or presses
// nothing: `e2e_chat_target` stages the window a dictation goes into (its
// process, title, the text on screen, where the focus is, whether an app's
// "@" list shows the one typed), and `e2e_dictation` returns the steps Yap
// would take (`steps`). The real windows are the robot's
// (e2e-real/chat-editors.real.js).
import fs from 'node:fs';
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';

const test = base.extend({
  yapOptions: [async ({}, use) => use({ name: 'chat-editors' }), { scope: 'worker' }],
});

const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');

/** A key as the keyboard sends it, in a webview (Playwright stops at F12). */
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

/** Dictations go into a stand-in window (see the top). */
function target(yap, { exe = null, title = '', screen = null, focusClass = null, focusName = null, picks = false } = {}) {
  return yap.invoke('e2e_chat_target', { exe, title, screen, focusClass, focusName, picks });
}

const SLACK_SCREEN = 'general - Example Ltd - Slack\nRochelle Okafor\nPriyanka Raman\nZentrix launch thread';

test.afterEach(async ({ yap, main }) => {
  await target(yap).catch(() => {});
  await setConfig(yap, main, { stackedMessages: false, chatMentions: true, fileTags: true, autoSubmit: false, autoSubmitKey: 'enter', screenNames: true });
});

test('Settings → Dictation: several messages, mentions and file tags (light and dark)', async ({ yap, main, shot }) => {
  const cfg = await yap.invoke('get_config');
  expect(cfg.stackedMessages).toBe(false);
  expect(cfg.chatMentions).toBe(true);
  expect(cfg.fileTags).toBe(true);

  const dialog = await openSettings(main, 'Dictation');
  const stacked = dialog.locator('[data-setting="stacked-messages"]');
  const mentions = dialog.locator('[data-setting="chat-mentions"]');
  const tags = dialog.locator('[data-setting="file-tags"]');
  await stacked.scrollIntoViewIfNeeded();
  const switchOf = (row, name) => row.getByRole('button', { name });
  await expect(switchOf(stacked, 'Send several messages: say “new message”')).toHaveAttribute('aria-pressed', 'false');
  await expect(switchOf(mentions, 'Turn “at Sam” into a mention')).toHaveAttribute('aria-pressed', 'true');
  await expect(switchOf(tags, 'Tag files in AI chat boxes')).toHaveAttribute('aria-pressed', 'true');
  await expect(mentions).toContainText('In Slack and Discord');

  await switchOf(stacked, 'Send several messages: say “new message”').click();
  await expectStore(yap, 'config.json', (c) => c.stackedMessages === true);
  // The send key shows with it.
  await expect(dialog.locator('[data-setting="send-key"]')).toBeVisible();
  await switchOf(tags, 'Tag files in AI chat boxes').click();
  await expectStore(yap, 'config.json', (c) => c.fileTags === false);
  await shot(main, '01-settings-light');

  // Both need the names on screen.
  const screenNames = dialog.locator('[data-setting="screen-names"]').getByRole('button', { name: "Spell names the way they're written on screen" });
  await screenNames.click();
  await expectStore(yap, 'config.json', (c) => c.screenNames === false);
  await expect(switchOf(mentions, 'Turn “at Sam” into a mention')).toBeDisabled();
  await expect(mentions).toContainText('Needs “Spell names the way they’re written on screen”');
  await screenNames.click();
  await expectStore(yap, 'config.json', (c) => c.screenNames === true);

  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await stacked.scrollIntoViewIfNeeded();
  await shot(main, '02-settings-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Settings' search finds them.
  const search = dialog.getByRole('searchbox', { name: 'Search settings' });
  await search.fill('new message');
  await expect(dialog.getByRole('listbox', { name: 'Settings found' }).getByRole('option').first()).toContainText('Send several messages');
  await search.fill('');
  await closeSettings(main);
});

test('Stacked messages: each pasted and sent in a chat app that sends anyway; on lines of their own when it doesn’t', async ({ yap, main }) => {
  await target(yap, { exe: 'slack.exe', screen: SLACK_SCREEN });
  await setConfig(yap, main, { stackedMessages: true, autoSubmit: true });

  let r = await yap.invoke('e2e_dictation', { text: 'Hi Rochelle. New message. Are you free at three?' });
  expect(r.kind).toBe('chat app');
  expect(r.text).toBe('Hi Rochelle\nAre you free at three?');
  expect(r.steps).toEqual([{ paste: 'Hi Rochelle' }, { key: 'enter' }, { paste: 'Are you free at three?' }, { key: 'enter' }]);

  // A clause of its own counts too, and the next message gets its capital.
  r = await yap.invoke('e2e_dictation', { text: 'Sounds good, new message, see you then' });
  expect(r.steps.filter((s) => s.paste)).toEqual([{ paste: 'Sounds good' }, { paste: 'See you then' }]);

  // Inside a sentence it's dictated.
  r = await yap.invoke('e2e_dictation', { text: 'I got a new message from Priyanka.' });
  expect(r.steps ?? []).toEqual([]);
  expect(r.text).toBe('I got a new message from Priyanka');

  // Not sent (no auto-submit, no "press enter"): one paste, lines of their own.
  await setConfig(yap, main, { autoSubmit: false });
  r = await yap.invoke('e2e_dictation', { text: 'Hi Rochelle. New message. Are you free at three?' });
  expect(r.steps ?? []).toEqual([]);
  expect(r.submit ?? null).toBe(null);
  expect(r.text).toBe('Hi Rochelle.\nAre you free at three?');

  // "Press enter" at the end sends them, each with the send key set.
  await setConfig(yap, main, { autoSubmitKey: 'ctrlEnter' });
  r = await yap.invoke('e2e_dictation', { text: 'Hi Rochelle. New message. Are you free? Press enter.' });
  expect(r.steps).toEqual([{ paste: 'Hi Rochelle' }, { key: 'ctrlEnter' }, { paste: 'Are you free?' }, { key: 'ctrlEnter' }]);

  // Not a chat app: as said, one paste.
  await target(yap, { exe: 'notepad.exe' });
  await setConfig(yap, main, { autoSubmit: true });
  r = await yap.invoke('e2e_dictation', { text: 'Hi Rochelle. New message. Are you free?' });
  expect(r.steps ?? []).toEqual([]);
  expect(r.text).toContain('New message');

  // Off: as said.
  await target(yap, { exe: 'slack.exe' });
  await setConfig(yap, main, { stackedMessages: false });
  r = await yap.invoke('e2e_dictation', { text: 'Hi Rochelle. New message. Are you free?' });
  expect(r.steps ?? []).toEqual([]);
  expect(r.text).toContain('New message');
  // The log has counts, never the words.
  expect(log(yap)).not.toContain('Are you free');
});

test('@mentions: typed and picked from Slack’s list when it shows the person; plain @Name in Teams; nothing in WhatsApp', async ({ yap, main }) => {
  // Slack, its list showing Rochelle: typed, Tab.
  await target(yap, { exe: 'slack.exe', screen: SLACK_SCREEN, picks: true });
  let r = await yap.invoke('e2e_dictation', { text: 'At Rochelle, can you check the build?' });
  expect(r.text).toBe('@Rochelle, can you check the build?');
  expect(r.steps).toEqual([{ typed: '@Rochelle', picked: true }, { paste: ', can you check the build?' }]);

  // The list not showing her: typed, no key — "@Rochelle" stays as text.
  await target(yap, { exe: 'slack.exe', screen: SLACK_SCREEN, picks: false });
  r = await yap.invoke('e2e_dictation', { text: 'Thanks at Priyanka' });
  expect(r.steps).toEqual([{ paste: 'Thanks ' }, { typed: '@Priyanka', picked: false }]);

  // A place, or a name not on screen: as said.
  r = await yap.invoke('e2e_dictation', { text: 'See you at Rochelle’s tonight.' });
  expect(r.steps ?? []).toEqual([]);
  r = await yap.invoke('e2e_dictation', { text: 'At Olivia, can you look?' });
  expect(r.text).toBe('At Olivia, can you look?');

  // Slack in a browser, by its tab's title.
  await target(yap, { exe: 'msedge.exe', title: 'general - Example Ltd - Slack - Microsoft​ Edge', screen: SLACK_SCREEN, picks: true });
  r = await yap.invoke('e2e_dictation', { text: 'At Rochelle, ship it.' });
  expect(r.steps[0]).toEqual({ typed: '@Rochelle', picked: true });

  // Teams: its list takes Enter, which could send — plain @Rochelle, one paste.
  await target(yap, { exe: 'ms-teams.exe', screen: SLACK_SCREEN, picks: true });
  r = await yap.invoke('e2e_dictation', { text: 'At Rochelle, can you check the build?' });
  expect(r.steps ?? []).toEqual([]);
  expect(r.text).toBe('@Rochelle, can you check the build?');

  // WhatsApp: no mentions.
  await target(yap, { exe: 'whatsapp.exe', screen: SLACK_SCREEN, picks: true });
  r = await yap.invoke('e2e_dictation', { text: 'At Rochelle, can you check the build?' });
  expect(r.text).toBe('At Rochelle, can you check the build?');

  // Off: as said.
  await target(yap, { exe: 'slack.exe', screen: SLACK_SCREEN, picks: true });
  await setConfig(yap, main, { chatMentions: false });
  r = await yap.invoke('e2e_dictation', { text: 'At Rochelle, can you check the build?' });
  expect(r.text).toBe('At Rochelle, can you check the build?');
  expect(r.steps ?? []).toEqual([]);

  // Mentions inside stacked messages: each message's own steps.
  await setConfig(yap, main, { chatMentions: true, stackedMessages: true, autoSubmit: true });
  r = await yap.invoke('e2e_dictation', { text: 'At Rochelle, hi. New message. Are you free?' });
  expect(r.steps).toEqual([
    { typed: '@Rochelle', picked: true },
    { paste: ', hi' },
    { key: 'enter' },
    { paste: 'Are you free?' },
    { key: 'enter' },
  ]);
});

const EDITOR_SCREEN =
  'userService.ts - Yap - Visual Studio Code\nuserService.ts, preview\nindex.ts\nREADME.md\n' +
  'export async function getUserById(id: string) { return retry(MAX_RETRY_COUNT, () => db.find(id)); }';

test('Code names: a file or a symbol said aloud is written as the code has it, in an editor', async ({ yap }) => {
  await target(yap, {
    exe: 'Code.exe',
    title: 'userService.ts - Yap - Visual Studio Code',
    screen: EDITOR_SCREEN,
    focusClass: 'inputarea monaco-mouse-cursor-text',
    focusName: 'Editor content;Press Alt+F1 for Accessibility Options.',
  });
  let r = await yap.invoke('e2e_dictation', { text: 'Call get user by id in user service dot ts' });
  expect(r.text).toBe('Call getUserById in userService.ts');
  // In the code editor: never a tag.
  expect(r.steps ?? []).toEqual([]);
  r = await yap.invoke('e2e_dictation', { text: 'set max retry count to five' });
  expect(r.text).toBe('set MAX_RETRY_COUNT to five');
  // Everyday words stay.
  r = await yap.invoke('e2e_dictation', { text: 'Get the user by Friday' });
  expect(r.text).toBe('Get the user by Friday');
});

test('File tags: picked in Cursor’s and VS Code’s AI chat, @path in Claude Code, never in the editor', async ({ yap, main }) => {
  // Cursor's chat box.
  await target(yap, { exe: 'Cursor.exe', title: 'userService.ts - Yap - Cursor', screen: EDITOR_SCREEN, focusClass: 'aislash-editor-input', picks: true });
  let r = await yap.invoke('e2e_dictation', { text: 'Fix the bug in user service dot ts' });
  expect(r.text).toBe('Fix the bug in @userService.ts');
  expect(r.steps).toEqual([{ paste: 'Fix the bug in ' }, { typed: '@userService.ts', picked: true }]);

  // VS Code's chat: "#" for a file.
  await target(yap, {
    exe: 'Code.exe',
    title: 'userService.ts - Yap - Visual Studio Code',
    screen: EDITOR_SCREEN,
    focusClass: 'inputarea monaco-mouse-cursor-text',
    focusName: 'Chat Input, Type your request',
    picks: true,
  });
  r = await yap.invoke('e2e_dictation', { text: 'Explain readme dot md' });
  expect(r.steps).toEqual([{ paste: 'Explain ' }, { typed: '#README.md', picked: true }]);

  // Claude Code in Windows Terminal: the path its screen showed.
  await target(yap, {
    exe: 'WindowsTerminal.exe',
    title: '✳ Fix the build',
    screen: '● Read(src/services/userService.ts)\n  ⎿  Read 120 lines\n> ',
  });
  r = await yap.invoke('e2e_dictation', { text: 'Fix the bug in user service dot ts' });
  expect(r.kind).toBe('terminal (Claude Code in Windows Terminal)');
  expect(r.text).toBe('Fix the bug in @src/services/userService.ts');
  expect(r.steps ?? []).toEqual([]);

  // Off: the name as written, no tag.
  await target(yap, { exe: 'Cursor.exe', title: 'userService.ts - Yap - Cursor', screen: EDITOR_SCREEN, focusClass: 'aislash-editor-input', picks: true });
  await setConfig(yap, main, { fileTags: false });
  r = await yap.invoke('e2e_dictation', { text: 'Fix the bug in user service dot ts' });
  expect(r.text).toBe('Fix the bug in userService.ts');
  expect(r.steps ?? []).toEqual([]);
});

test('Settings → Shortcuts: Send is unbound until set, never Enter, and presses the send key', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Shortcuts');
  const send = dialog.locator('[data-setting="shortcut-send"]');
  await send.scrollIntoViewIfNeeded();
  await expect(send).toContainText('Send');
  await expect(send.getByRole('button', { name: 'Set a shortcut' })).toBeVisible();
  expect((await yap.invoke('get_config')).sendHotkey ?? '').toBe('');

  // Enter refused; the dictation hotkey (F24 in the suite) refused; F19 taken.
  await send.getByRole('button', { name: 'Set a shortcut' }).click();
  await pressKey(main, 'Enter', 13);
  await expect(send).toContainText('Enter sends already');
  await pressKey(main, 'F24', 135);
  await expect(send).toContainText('Already used by the dictation hotkey');
  await pressKey(main, 'F19', 130);
  await expectStore(yap, 'config.json', (c) => c.sendHotkey === 'kb:130');
  await expect.poll(() => yap.invoke('e2e_hotkey_bindings').then((b) => b.actions.sendHotkey[0].key)).not.toBe('none');
  await shot(main, '03-shortcuts-send');

  // Its press, as the hook sends it: the send key set (Enter in a terminal).
  let mark = log(yap).length;
  await yap.invoke('e2e_shortcut', { name: 'send' });
  await expect.poll(() => log(yap).slice(mark)).toContain('e2e: not pressing the send key in another app');
  expect(log(yap).slice(mark)).toMatch(/key="?enter"?/);
  await setConfig(yap, main, { autoSubmitKey: 'shiftEnter' });
  mark = log(yap).length;
  await yap.invoke('e2e_shortcut', { name: 'send' });
  await expect.poll(() => log(yap).slice(mark)).toMatch(/key="?shiftEnter"?/);
  await target(yap, { exe: 'WindowsTerminal.exe', title: 'pwsh' });
  mark = log(yap).length;
  await yap.invoke('e2e_shortcut', { name: 'send' });
  await expect.poll(() => log(yap).slice(mark)).toMatch(/key="?enter"?.*e2e: not pressing the send key|e2e: not pressing the send key.*key="?enter"?/);

  // Removed again.
  await openSettings(main, 'Shortcuts');
  await send.getByRole('button', { name: 'Remove hotkey' }).click();
  await expectStore(yap, 'config.json', (c) => c.sendHotkey === '');
  await expect.poll(() => yap.invoke('e2e_hotkey_bindings').then((b) => b.actions.sendHotkey[0].key)).toBe('none');
  await closeSettings(main);
});
