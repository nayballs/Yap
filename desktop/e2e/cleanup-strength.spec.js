// Cleanup strength, smart formatting, a transform on every dictation and the
// text before the cursor (ai/cleanup_strength.rs, ai/cleanup_format.rs,
// dictation/transforms.rs `for_dictation`, ai/cleanup_context.rs; 2026-10-25):
// - Settings → Language Models → Dictation cleanup → Strength (None / Light /
//   Medium / High): a config from before shows Medium with cleanup on, None
//   with it off; None turns cleanup off, a strength turns it back on.
// - What a dictation's cleanup is told, through the fake AI: Light's and
//   High's lines just before the transcript, none at Medium (the prompt as
//   before), none for a tone that keeps the words (a terminal, AI prompts);
//   at Light an email keeps its kind but not its formal tone; a custom body
//   stays as written. A spoken list or an email's greeting gets its layout
//   line, a plain sentence none, and a list the model kept on one line is
//   laid out. "Use what I said" is still offered at High.
// - Settings → Commands → Transforms → "Run on every dictation": the
//   transform runs once after cleanup, only in the kind of app picked, never
//   in a terminal.
// - The text before the cursor: sent to a cleanup model on this PC; to a
//   cloud one (the fake standing in for api.groq.com through the debug-only
//   YAP_E2E_NET_ALIASES) only with Privacy & data's switch on, which its row
//   in "What leaves this PC" says, and the network receipts name it.
//
// A test run reads no other app's window: dictations go to a stand-in app
// (`e2e_style_app`) with a stand-in text before the cursor
// (`e2e_cursor_before`). Fictional people and details only.
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig, settingsDialog } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript, transformText } from './support/fake-llm.js';

const LIGHT = 'Light cleanup: only remove filler sounds';
const HIGH = 'Thorough cleanup: also make it clear and concise';
const NUMBERED = 'The speaker may be dictating a numbered list';
const EMAIL = 'Put the greeting on its own line';
const CURSOR = 'Where it goes: right after "';
const GROQ_KEY = 'gsk_FAKEstrength0123456789abcXYZ';

const test = base.extend({
  fakeLocal: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeCloud: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLocal, fakeCloud }, use) => {
      // A config from before strengths: cleanup on, no `cleanupStrength`.
      await use({
        name: 'cleanup-strength',
        env: { YAP_E2E_GPU: 'none', YAP_E2E_NET_ALIASES: `${new URL(fakeCloud.base).host}=api.groq.com` },
        config: { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: fakeLocal.base, ppModel: 'fake-cleanup', ppApiKey: '' },
      });
    },
    { scope: 'worker' },
  ],
});

const lastOf = (llm, kind) => llm.requests.filter((r) => r.kind === kind).at(-1);

async function dictate(yap, exe, title, text) {
  await yap.invoke('e2e_style_app', { exe, title });
  return yap.invoke('e2e_dictation', { text });
}

test('Settings: a config from before cleans at Medium; None turns cleanup off and a strength back on (light and dark)', async ({ yap, main, shot }) => {
  // The file had no strength: Medium, what cleanup always did.
  expect((await yap.invoke('get_config')).cleanupStrength).toBe('medium');

  const dialog = await openSettings(main, 'Language Models');
  const row = dialog.locator('[data-setting="cleanup-strength"]');
  const strength = row.getByRole('tablist', { name: 'Cleanup strength' });
  await expect(strength.getByRole('tab', { name: 'Medium' })).toHaveAttribute('aria-selected', 'true');
  await expect(row).toContainText('Also fixes grammar and false starts, and lays out spoken lists');
  await shot(main, '01-strength-medium');

  // Light: saved, cleanup still on.
  await strength.getByRole('tab', { name: 'Light' }).click();
  await expectStore(yap, 'config.json', (c) => c.cleanupStrength === 'light' && c.postProcessEnabled === true);
  await expect(row).toContainText('Punctuation, capitals and filler sounds only: your words stay');

  // None: cleanup off (the strength kept for next time), the page's off line.
  await strength.getByRole('tab', { name: 'None' }).click();
  await expectStore(yap, 'config.json', (c) => c.postProcessEnabled === false && c.cleanupStrength === 'light');
  await expect(dialog.getByRole('button', { name: 'Clean up dictations with AI' })).toHaveAttribute('aria-pressed', 'false');
  await expect(dialog.getByText('Turn on cleanup to pick a model.')).toBeVisible();
  await shot(main, '02-strength-none');

  // High: back on.
  await strength.getByRole('tab', { name: 'High' }).click();
  await expectStore(yap, 'config.json', (c) => c.postProcessEnabled === true && c.cleanupStrength === 'high');
  await expect(row).toContainText('Also tightens and rephrases for clarity, keeping your meaning and voice');
  // The switch off and on keeps it.
  await dialog.getByRole('button', { name: 'Clean up dictations with AI' }).click();
  await expect(strength.getByRole('tab', { name: 'None' })).toHaveAttribute('aria-selected', 'true');
  await dialog.getByRole('button', { name: 'Clean up dictations with AI' }).click();
  await expect(strength.getByRole('tab', { name: 'High' })).toHaveAttribute('aria-selected', 'true');
  await expectStore(yap, 'config.json', (c) => c.postProcessEnabled === true && c.cleanupStrength === 'high');

  await setConfig(yap, main, { theme: 'dark' });
  await row.scrollIntoViewIfNeeded();
  await shot(main, '03-strength-high-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Search finds it.
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await search.fill('strength');
  await expect(settingsDialog(main).getByRole('listbox', { name: 'Settings found' }).getByRole('option').filter({ hasText: 'Strength' })).toBeVisible();
  await search.fill('');
  await closeSettings(main);
  await setConfig(yap, main, { cleanupStrength: 'medium' });
});

test('each strength reaches the model as its own line; Medium sends the prompt as before; lists and emails get their layout', async ({ yap, main, fakeLocal: llm }) => {
  const cleanup = () => lastOf(llm, 'cleanup');
  try {
    // Medium (Notepad: everything else): no line of ours, as before.
    let r = await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'so the plan is fine');
    expect(r.style).toBe('other/natural');
    for (const l of [LIGHT, HIGH, NUMBERED, EMAIL, CURSOR]) expect(cleanup().user).not.toContain(l);
    const mediumSystem = cleanup().system;

    // Light: its line just before the transcript; the system prompt the same.
    await setConfig(yap, main, { cleanupStrength: 'light' });
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'so the plan is fine');
    expect(cleanup().user).toContain(LIGHT);
    expect(cleanup().user.indexOf(LIGHT)).toBeLessThan(cleanup().user.indexOf('Transcript:'));
    expect(cleanup().system).toBe(mediumSystem);
    // An email at Light: still the body of an email, but no formal rewording.
    r = await dictate(yap, 'OUTLOOK.EXE', 'Inbox - Outlook', "hi sam i'll send it tomorrow");
    expect(r.style).toBe('email/natural');
    expect(cleanup().user).toContain('This dictation is the body of an email.');
    expect(cleanup().user).not.toContain('Tone: formal');
    // Light lays nothing out.
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'my priorities are one finish the budget two hire the designer three fix the bug');
    expect(cleanup().user).not.toContain(NUMBERED);

    // High: its line; never for a terminal command or an AI prompt.
    await setConfig(yap, main, { cleanupStrength: 'high' });
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'so the plan is fine');
    expect(cleanup().user).toContain(HIGH);
    r = await dictate(yap, 'powershell.exe', 'Windows PowerShell', 'git status');
    expect(r.style).toBe('terminal/command');
    expect(cleanup().user).not.toContain(HIGH);
    r = await dictate(yap, 'ChatGPT.exe', 'ChatGPT', 'write a function that reverses a string');
    expect(r.style).toBe('aiPrompts/literal');
    expect(cleanup().user).not.toContain(HIGH);

    // "Use what I said" is still there when High changed the words.
    llm.reply('cleanup', () => 'The plan works.');
    r = await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'so um the plan is basically fine i think');
    expect(r.text.trim()).toBe('The plan works.');
    expect(r.asSaid?.trim()).toBe('so um the plan is basically fine i think');
    llm.reply('cleanup', cleanupTranscript);

    // A custom body stays exactly as written; the strength rides beside it.
    await setConfig(yap, main, { ppPreset: 'custom', ppPrompt: 'Write it the way a ship captain would.' });
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'we sail at dawn');
    expect(cleanup().system).toContain('Write it the way a ship captain would.');
    expect(cleanup().system).not.toContain(HIGH);
    expect(cleanup().user).toContain(HIGH);
    await setConfig(yap, main, { ppPreset: 'default', cleanupStrength: 'medium' });

    // Medium: a spoken list gets its line, a plain sentence with numbers none.
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'my priorities are one finish the budget two hire the designer three fix the bug');
    expect(cleanup().user).toContain(NUMBERED);
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'i have one cat two dogs and three fish');
    expect(cleanup().user).not.toContain(NUMBERED);
    // An email's greeting and sign-off: its layout line, in an email only.
    await dictate(yap, 'OUTLOOK.EXE', 'Inbox - Outlook', 'hi sam thanks for the figures cheers');
    expect(cleanup().user).toContain(EMAIL);
    await dictate(yap, 'slack.exe', 'general - Acme - Slack', 'hi sam thanks for the figures cheers');
    expect(cleanup().user).not.toContain(EMAIL);

    // A list the model kept on one line is laid out (a small model does).
    llm.reply('cleanup', () => 'For the offsite we need: first, book the venue; second, sort out the catering; third, send the invites.');
    r = await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'for the offsite we need first book the venue second sort out the catering third send the invites');
    expect(r.text.trim()).toBe('For the offsite we need:\n1. Book the venue\n2. Sort out the catering\n3. Send the invites');
    // And an email kept on one line gets its greeting and sign-off lines.
    llm.reply('cleanup', () => 'Hi Sam, thanks for the figures. Cheers.');
    r = await dictate(yap, 'OUTLOOK.EXE', 'Inbox - Outlook', 'hi sam thanks for the figures cheers');
    expect(r.text.trim()).toBe('Hi Sam,\n\nThanks for the figures.\n\nCheers,');
  } finally {
    llm.reply('cleanup', cleanupTranscript);
    await yap.invoke('e2e_style_app', { exe: null, title: null });
    await setConfig(yap, main, { cleanupStrength: 'medium', ppPreset: 'default' });
  }
});

test('a transform on every dictation runs once after cleanup, in the apps picked, never in a terminal', async ({ yap, main, shot, fakeLocal: llm }) => {
  llm.reply('transform', (user) => `Polished: ${transformText(user)}`);
  const transforms = () => llm.requests.filter((r) => r.kind === 'transform').length;
  try {
    const dialog = await openSettings(main, 'Commands');
    const row = dialog.locator('[data-setting="transform-every"]');
    await row.scrollIntoViewIfNeeded();
    await expect(row).toContainText('Off. Pick a transform to run after cleanup on what you dictate');
    await expect(row.getByLabel('Where it runs')).toBeDisabled();
    await row.getByLabel('Transform on every dictation').selectOption('polish');
    await row.getByLabel('Where it runs').selectOption('email');
    await expectStore(yap, 'config.json', (c) => c.autoTransform === 'polish' && c.autoTransformKind === 'email');
    await expect(row).toContainText('Polish runs after cleanup on each dictation in email, never in a terminal');
    await shot(main, '04-transform-every-dictation');
    // Privacy & data lists where it goes while one is picked.
    const privacy = await openSettings(main, 'Privacy & data');
    await expect(
      privacy.getByRole('table', { name: 'What leaves this PC' }).getByRole('row').filter({ has: main.getByRole('rowheader', { name: 'Transform on every dictation' }) }),
    ).toContainText('Your dictations, as text, go to your custom endpoint');
    await closeSettings(main);

    // Outlook: cleanup, then Polish once on what cleanup wrote.
    let before = transforms();
    let r = await dictate(yap, 'OUTLOOK.EXE', 'Inbox - Outlook', 'the invoice was paid this morning');
    expect(transforms()).toBe(before + 1);
    const t = lastOf(llm, 'transform');
    expect(transformText(t.user)).toBe(cleanupTranscript(lastOf(llm, 'cleanup').user));
    expect(r.text.trim()).toBe('Polished: the invoice was paid this morning');
    // "Use what I said" puts back the words as said.
    expect(r.asSaid?.trim()).toBe('the invoice was paid this morning');

    // Slack: another kind, no transform.
    before = transforms();
    r = await dictate(yap, 'slack.exe', 'general - Acme - Slack', 'running five minutes late');
    expect(transforms()).toBe(before);
    expect(r.text).not.toContain('Polished');

    // Every app: Slack too; a terminal never.
    await setConfig(yap, main, { autoTransformKind: '' });
    before = transforms();
    r = await dictate(yap, 'slack.exe', 'general - Acme - Slack', 'running five minutes late');
    expect(transforms()).toBe(before + 1);
    expect(r.text).toContain('Polished: running five minutes late');
    before = transforms();
    r = await dictate(yap, 'powershell.exe', 'Windows PowerShell', 'git status');
    expect(transforms()).toBe(before);
    expect(r.text.trim()).toBe('git status');

    // A failure: the cleaned text, as if no transform were picked.
    llm.failNext('transform', 1);
    r = await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'see you at noon');
    expect(r.text.trim()).toBe('see you at noon');
  } finally {
    await yap.invoke('e2e_style_app', { exe: null, title: null });
    await setConfig(yap, main, { autoTransform: '', autoTransformKind: '' });
  }
});

test('the text before the cursor goes to a cleanup model on this PC, and to a cloud one only with the switch on', async ({ yap, main, shot, fakeLocal, fakeCloud }) => {
  await yap.invoke('e2e_cursor_before', { text: 'Could you send me the' });
  try {
    // The fake on 127.0.0.1: a model on this PC.
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'send me the final report by friday');
    expect(lastOf(fakeLocal, 'cleanup').user).toContain(`${CURSOR}Could you send me the"`);
    expect(lastOf(fakeLocal, 'cleanup').user.indexOf(CURSOR)).toBeLessThan(lastOf(fakeLocal, 'cleanup').user.indexOf('Transcript:'));
    expect(lastOf(fakeLocal, 'cleanup').system).not.toContain('Could you send me the');

    // Groq (the cloud fake): not without the switch.
    await setConfig(yap, main, { ppProvider: 'groq', ppBaseUrl: fakeCloud.base, ppModel: 'openai/gpt-oss-20b', ppApiKey: GROQ_KEY, ppApiKeys: { groq: GROQ_KEY } });
    await yap.invoke('connections_clear');
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'send me the final report by friday');
    expect(lastOf(fakeCloud, 'cleanup').user).not.toContain(CURSOR);
    expect(JSON.stringify(await yap.invoke('connections_summary'))).toContain('AI cleanup · Groq');

    // Privacy & data says so; the switch on, it goes, and says so.
    const dialog = await openSettings(main, 'Privacy & data');
    const table = dialog.getByRole('table', { name: 'What leaves this PC' });
    const row = table.getByRole('row').filter({ has: main.getByRole('rowheader', { name: 'Text before the cursor' }) });
    await expect(row).toContainText('Read on this PC for each dictation, never sent');
    const toggle = dialog.getByRole('button', { name: 'Send the text before the cursor to cleanup' });
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await expect(dialog.locator('[data-setting="cursor-context"]')).toContainText('goes to Groq with each dictation');
    await toggle.click();
    await expectStore(yap, 'config.json', (c) => c.cursorContextCloud === true);
    await expect(row).toContainText('A sentence or two before the cursor goes to Groq with each dictation, for cleanup');
    await toggle.scrollIntoViewIfNeeded();
    await shot(main, '05-privacy-text-before-cursor');
    await closeSettings(main);

    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'send me the final report by friday');
    expect(lastOf(fakeCloud, 'cleanup').user).toContain(`${CURSOR}Could you send me the"`);
    // The network receipts name what went.
    expect(JSON.stringify(await yap.invoke('connections_summary'))).toContain('AI cleanup + text before the cursor · Groq');

    // On this PC the switch doesn't matter (greyed), and it still goes.
    await setConfig(yap, main, { ppProvider: 'custom', ppBaseUrl: fakeLocal.base, ppModel: 'fake-cleanup', ppApiKey: '', cursorContextCloud: false });
    await dictate(yap, 'notepad.exe', 'notes.txt - Notepad', 'send me the final report by friday');
    expect(lastOf(fakeLocal, 'cleanup').user).toContain(CURSOR);
    const page = await openSettings(main, 'Privacy & data');
    await expect(page.getByRole('button', { name: 'Send the text before the cursor to cleanup' })).toBeDisabled();
    await expect(
      page.getByRole('table', { name: 'What leaves this PC' }).getByRole('row').filter({ has: main.getByRole('rowheader', { name: 'Text before the cursor' }) }),
    ).toContainText('goes with it to the cleanup model on this PC, never sent');
    await closeSettings(main);
  } finally {
    await yap.invoke('e2e_cursor_before', { text: null });
    await yap.invoke('e2e_style_app', { exe: null, title: null });
    await setConfig(yap, main, { ppProvider: 'custom', ppBaseUrl: fakeLocal.base, ppModel: 'fake-cleanup', ppApiKey: '', cursorContextCloud: false });
  }
});
