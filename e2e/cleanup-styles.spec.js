// Styles by app (ai/cleanup_styles.rs, 2026-10-24): AI cleanup in the right
// tone for where you are — Settings → Language Models → Dictation cleanup →
// "Styles by app" (settings/CleanupStyles.svelte): a row per kind of app
// with its tone, an example, its apps and extra instructions — and what a
// dictation's cleanup is told in each kind of app: the kind's line just
// before the transcript, none for everything else, the main cleanup alone
// with the switch off, and an app rule of the person's own winning as
// before. A filler-only dictation asks no model. Onboarding recommends
// cleanup in the cloud on a PC without a graphics card for the on-device
// model (cleanup-offer.spec.js has the PC with one, and the offer).
//
// A test run reads no other app's window: dictations go to a stand-in app
// (`e2e_style_app`: a process name and a title, as a real window would
// give). Real windows are the robot's (e2e-real/cleanup-styles.real.js).
import { test as base, expect, openSettings, closeSettings, expectStore, setConfig, settingsDialog } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript } from './support/fake-llm.js';

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({ name: 'cleanup-styles', env: { YAP_E2E_GPU: 'none' } });
    },
    { scope: 'worker' },
  ],
});

/** Clean up on, through the fake model. */
async function cleanupOn(yap, main, llm, patch = {}) {
  await setConfig(yap, main, {
    postProcessEnabled: true,
    ppProvider: 'custom',
    ppBaseUrl: llm.base,
    ppModel: 'fake-cleanup',
    ppApiKey: '',
    ...patch,
  });
}

const styleOf = (cfg, kind) => (cfg.cleanupProfiles || []).find((p) => p.category === kind);

test('Settings: a row per kind of app with its tone and an example; a tone, an app and instructions are saved (light and dark)', async ({
  yap,
  main,
  shot,
}) => {
  const llm = await startFakeLlm();
  await cleanupOn(yap, main, llm);
  // Every kind's style is there from the first load, with its default tone.
  const cfg = await yap.invoke('get_config');
  expect(cfg.cleanupStyles).toBe(true);
  expect(styleOf(cfg, 'email').tone).toBe('formal');
  expect(styleOf(cfg, 'workChat').tone).toBe('casual');
  expect(styleOf(cfg, 'aiPrompts').tone).toBe('literal');
  expect(styleOf(cfg, 'terminal').tone).toBe('command');
  expect(styleOf(cfg, 'other').tone).toBe('natural');

  const dialog = await openSettings(main, 'Language Models');
  const group = dialog.getByRole('group', { name: 'Styles by app' });
  await group.scrollIntoViewIfNeeded();
  await expect(group.getByRole('button', { name: 'Match the style to the app' })).toHaveAttribute('aria-pressed', 'true');
  const kinds = group.locator('section.kind');
  await expect(kinds).toHaveCount(7);
  await expect(kinds.locator('h3')).toHaveText(['Email', 'Work chat', 'Personal chat', 'Documents', 'AI prompts', 'Terminal', 'Everything else']);

  const email = group.locator('section[data-kind="email"]');
  await expect(email.locator('.apps')).toHaveText(/^Outlook, Gmail, Thunderbird and \d+ more$/);
  await expect(email.getByRole('tab', { name: 'Formal' })).toHaveAttribute('aria-selected', 'true');
  await expect(email.locator('.typed-text')).toContainText('I will send the updated proposal');
  await email.scrollIntoViewIfNeeded();
  await shot(main, '01-styles-by-app');

  // A tone: saved, and the example follows it.
  await email.getByRole('tab', { name: 'Casual' }).click();
  await expectStore(yap, 'config.json', (c) => styleOf(c, 'email')?.tone === 'casual');
  await expect(email.locator('.typed-text')).toContainText("I'll send the updated proposal");

  // AI prompts: literal, word for word.
  const ai = group.locator('section[data-kind="aiPrompts"]');
  await expect(ai.getByRole('tab', { name: 'Literal' })).toHaveAttribute('aria-selected', 'true');
  await expect(ai.locator('.tone-line')).toHaveText('Your words in your order: nothing added, nothing rephrased, never answered');

  // Apps: Discord moved from personal chat to work chat; a site added to Documents.
  const work = group.locator('section[data-kind="workChat"]');
  await work.locator('summary').click();
  await work.getByLabel('Add an app to Work chat').fill('discord.exe');
  await work.getByRole('button', { name: 'Add', exact: true }).click();
  await expectStore(
    yap,
    'config.json',
    (c) => styleOf(c, 'workChat')?.apps?.includes('discord.exe') && styleOf(c, 'personalChat')?.removedApps?.includes('Discord'),
  );
  await expect(work.getByRole('list', { name: 'Work chat apps' })).toContainText('Discord');
  const personal = group.locator('section[data-kind="personalChat"]');
  await expect(personal.locator('.apps')).not.toContainText('Discord');

  const docs = group.locator('section[data-kind="documents"]');
  await docs.locator('summary').click();
  await docs.getByLabel('Add an app to Documents').fill('Basecamp');
  await docs.getByLabel('Add an app to Documents').press('Enter');
  await docs.getByRole('textbox', { name: 'Extra instructions for Documents' }).fill('Use British date formats.');
  await expectStore(
    yap,
    'config.json',
    (c) => styleOf(c, 'documents')?.apps?.includes('Basecamp') && styleOf(c, 'documents')?.prompt === 'Use British date formats.',
  );
  // Take Word out, then put it back.
  await docs.getByRole('button', { name: 'Take Word out of Documents' }).click();
  await expectStore(yap, 'config.json', (c) => styleOf(c, 'documents')?.removedApps?.includes('Word'));
  await docs.getByRole('button', { name: 'Put Word back in Documents' }).click();
  await expectStore(yap, 'config.json', (c) => !styleOf(c, 'documents')?.removedApps?.includes('Word'));
  await docs.scrollIntoViewIfNeeded();
  await shot(main, '02-styles-apps-and-instructions');

  // Dark.
  await setConfig(yap, main, { theme: 'dark' });
  await email.scrollIntoViewIfNeeded();
  await shot(main, '03-styles-by-app-dark');
  await setConfig(yap, main, { theme: 'light' });

  // The person's own profiles stay theirs: the styles aren't in that list,
  // and an app rule can name one.
  const routing = dialog.getByRole('group', { name: 'Smart routing' });
  await expect(routing.getByText('No profiles yet.')).toBeVisible();

  // Off: no rows; every app gets the main cleanup.
  await group.getByRole('button', { name: 'Match the style to the app' }).click();
  await expectStore(yap, 'config.json', (c) => c.cleanupStyles === false);
  await expect(group.locator('section.kind')).toHaveCount(0);
  await group.getByRole('button', { name: 'Match the style to the app' }).click();
  await expectStore(yap, 'config.json', (c) => c.cleanupStyles === true);

  // Search finds it.
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await search.fill('formal casual');
  await expect(settingsDialog(main).getByRole('listbox', { name: 'Settings found' }).getByRole('option').filter({ hasText: 'Match the style to the app' })).toBeVisible();
  await search.fill('');
  await closeSettings(main);
  // Back to the defaults for the next test.
  const now = await yap.invoke('get_config');
  await setConfig(yap, main, {
    cleanupProfiles: now.cleanupProfiles.map((p) =>
      p.category ? { ...p, tone: { email: 'formal' }[p.category] ?? p.tone, apps: [], removedApps: [], prompt: '' } : p,
    ),
  });
});

test("a dictation's cleanup is told the kind of app it goes into, and written in its tone", async ({ yap, main }) => {
  const llm = await startFakeLlm();
  await cleanupOn(yap, main, llm);
  const lastCleanup = () => llm.requests.filter((r) => r.kind === 'cleanup').at(-1);
  const dictate = async (exe, title, text) => {
    await yap.invoke('e2e_style_app', { exe, title });
    return yap.invoke('e2e_dictation', { text });
  };
  try {
    // Outlook: the email's line, just before the transcript; the main body.
    let r = await dictate('OUTLOOK.EXE', 'Inbox - Outlook', 'hi sam thanks for the figures');
    expect(r.style).toBe('email/formal');
    let req = lastCleanup();
    expect(req.user).toContain('This dictation is the body of an email. Tone: formal and polished');
    expect(req.user.indexOf('This dictation is the body of an email')).toBeLessThan(req.user.indexOf('Transcript:'));
    expect(req.system).toContain('Clean up the transcript using these rules.');
    // The style's line is in the dictation's own message, never the system
    // prompt: the long prompt every kind shares stays the same.
    expect(req.system).not.toContain('This dictation is');

    // Gmail in a browser: by the site its title ends with.
    r = await dictate('chrome.exe', 'Inbox (2) - sam@example.com - Gmail - Google Chrome', 'see you thursday');
    expect(r.style).toBe('email/formal');

    // Slack: casual.
    r = await dictate('slack.exe', 'general - Acme - Slack', 'running five minutes late');
    expect(r.style).toBe('workChat/casual');
    expect(lastCleanup().user).toContain('This dictation is a work chat message. Tone: casual and friendly');

    // ChatGPT: literal, no "please".
    r = await dictate('ChatGPT.exe', 'ChatGPT', 'write a function that reverses a string');
    expect(r.style).toBe('aiPrompts/literal');
    expect(lastCleanup().user).toContain('never add words (no please or thanks)');

    // A terminal: a command, with no capital or full stop added even when
    // the model adds them.
    llm.reply('cleanup', () => 'Git status.');
    r = await dictate('powershell.exe', 'Windows PowerShell', 'git status');
    expect(r.style).toBe('terminal/command');
    expect(r.text.trim()).toBe('git status');
    llm.reply('cleanup', cleanupTranscript);

    // Notepad: everything else, in your own words — no line, as before.
    r = await dictate('notepad.exe', 'notes.txt - Notepad', 'so the plan is fine');
    expect(r.style).toBe('other/natural');
    expect(lastCleanup().user).not.toContain('This dictation is');

    // Filler alone: no model asked, nothing pasted.
    const before = llm.requests.length;
    r = await dictate('slack.exe', 'Slack', 'um uh');
    expect(r.text).toBe('');
    expect(llm.requests.length).toBe(before);

    // Terminal "As said": no AI at all.
    const cfg = await yap.invoke('get_config');
    await setConfig(yap, main, { cleanupProfiles: cfg.cleanupProfiles.map((p) => (p.category === 'terminal' ? { ...p, tone: 'off' } : p)) });
    const asked = llm.requests.length;
    r = await dictate('powershell.exe', 'Windows PowerShell', 'Git status.');
    expect(r.style ?? null).toBe(null);
    expect(llm.requests.length).toBe(asked);

    // The person's own app rule wins, exactly as before styles.
    const now = await yap.invoke('get_config');
    await setConfig(yap, main, {
      cleanupProfiles: [
        ...now.cleanupProfiles.map((p) => (p.category === 'terminal' ? { ...p, tone: 'command' } : p)),
        { id: 'mine', name: 'Terse', prompt: 'Be terse.', provider: '', baseUrl: '', model: '', apiKey: '' },
      ],
      appRoutes: [{ app: 'slack.exe', label: 'Slack', profileId: 'mine', prompt: '' }],
    });
    r = await dictate('slack.exe', 'general - Acme - Slack', 'running five minutes late');
    expect(r.style ?? null).toBe(null);
    req = lastCleanup();
    expect(req.system).toContain('Be terse.');
    expect(req.user).not.toContain('This dictation is');

    // Styles off: the main cleanup everywhere, no line.
    await setConfig(yap, main, { cleanupStyles: false, appRoutes: [] });
    r = await dictate('OUTLOOK.EXE', 'Inbox - Outlook', 'hi sam thanks for the figures');
    expect(r.style ?? null).toBe(null);
    expect(lastCleanup().user).not.toContain('This dictation is');
  } finally {
    await yap.invoke('e2e_style_app', { exe: null, title: null });
    const now = await yap.invoke('get_config');
    await setConfig(yap, main, {
      cleanupStyles: true,
      appRoutes: [],
      cleanupProfiles: now.cleanupProfiles.filter((p) => p.category),
      postProcessEnabled: false,
    });
  }
});

test('onboarding: with no graphics card for the on-device model, cleanup in the cloud is recommended, with what goes there', async ({
  yap,
  main,
  shot,
}) => {
  await setConfig(yap, main, { postProcessEnabled: false });
  const onboarding = yap.onboarding;
  const dialog = await openSettings(main, 'About');
  await dialog.getByRole('button', { name: /Show setup guide again/ }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Welcome to Yap' })).toBeVisible();
  await onboarding.getByRole('button', { name: "I'll choose later" }).click();
  await onboarding.getByRole('button', { name: 'Continue →' }).click();
  await expect(onboarding.getByRole('heading', { level: 1, name: 'Make it sound polished' })).toBeVisible();
  const cloud = onboarding.locator('.mode-card[data-recommended="yes"]');
  await expect(cloud).toContainText('Cloud');
  await expect(cloud).toContainText('Recommended for this PC');
  await expect(cloud).toHaveClass(/sel/);
  await expect(onboarding.locator('.mode-card[data-recommended="no"]')).toContainText('slower without a graphics card');
  await expect(onboarding.locator('[data-privacy="cloud"]')).toContainText('Your voice never leaves this PC');
  // Nothing's on until the person picks: "Maybe later" leaves it off.
  expect((await yap.invoke('get_config')).postProcessEnabled).toBe(false);
  await shot(onboarding, '04-onboarding-recommends-cloud');
  await onboarding.getByRole('button', { name: 'Maybe later' }).click();
  expect((await yap.invoke('get_config')).postProcessEnabled).toBe(false);
  await yap.invoke('close_onboarding');
  await closeSettings(main);
});
