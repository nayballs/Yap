// Writing samples (ai/writing_samples.rs, 2026-10-29): Settings → Commands →
// Writing samples — added, tagged with a kind of app, edited, deleted behind
// the confirm dialog, kept in writing_samples.json (never config.json) —
// then what the AI is sent, through the fake model (support/fake-llm.js):
// - Polish and the rewrite shortcut carry the samples for the kind of app at
//   the end of the system prompt (the part a server caches), the user's turn
//   as ever; Fix grammar only, an AI prompt and a terminal never do;
// - a model on this PC gets fewer than a cloud one (the fake standing in for
//   api.groq.com through the debug-only YAP_E2E_NET_ALIASES), and the network
//   receipts say they went ("Commands + your writing samples · Groq");
// - AI cleanup never carries them: at Medium and High its request is
//   exactly the one with no samples;
// - with no samples, every request is as it was.
//
// A test run reads and pastes into no other app: dictations, transforms and
// rewrites go to a stand-in window (`e2e_target_window`) in a stand-in app
// (`e2e_style_app`). Every sample is made up.
import { test as base, expect, openSettings, closeSettings, settingsDialog, expectStore, setConfig } from './support/fixtures.js';
import { startFakeLlm, cleanupTranscript, transformText } from './support/fake-llm.js';

const BLOCK = 'How the user writes: examples of their own writing';
const GROQ_KEY = 'gsk_FAKEsamples0123456789abcXYZ';
const EMAIL_SAMPLE = 'Hiya all,\n\nQuick one – the allotment committee has moved to Thursday at 7.\n\nCheers,\nRobin';
const CHAT_SAMPLE = 'running a bit late – be there in 10, sorry';
const ANY_SAMPLE = 'Sounds good to me – let\'s go with the cheaper one and see how we get on.';
// Long enough that a model on this PC gets only some of them.
const LONG_EMAIL = `Hi both,\n\n${'Thanks for the plans – they look good to me, and the dates work. '.repeat(9)}\n\nCheers,\nRobin`;
const LONG_EMAIL2 = `Morning all,\n\n${'Quick update – the venue works for us, so let us book it soon. '.repeat(9)}\n\nCheers,\nRobin`;

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
      await use({
        name: 'writing-samples',
        env: { YAP_E2E_GPU: 'none', YAP_E2E_NET_ALIASES: `${new URL(fakeCloud.base).host}=api.groq.com` },
        config: { postProcessEnabled: true, ppProvider: 'custom', ppBaseUrl: fakeLocal.base, ppModel: 'fake-cleanup', ppApiKey: '' },
      });
    },
    { scope: 'worker' },
  ],
});

const lastOf = (llm, kind) => llm.requests.filter((r) => r.kind === kind).at(-1);

test('Settings → Commands → Writing samples: added, tagged, edited, deleted, kept in their own file (light and dark)', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Commands');
  const group = dialog.getByRole('group', { name: 'Writing samples' });
  await group.scrollIntoViewIfNeeded();
  await expect(group).toContainText('None yet');
  await shot(main, '01-writing-samples-empty');

  // Add one, tagged Email.
  await group.getByRole('button', { name: 'Add a writing sample' }).click();
  let form = group.getByRole('group', { name: 'New writing sample' });
  await expect(form.getByRole('textbox', { name: 'Writing sample' })).toBeFocused();
  await expect(form.getByRole('button', { name: 'Add sample' })).toBeDisabled();
  await form.getByRole('textbox', { name: 'Writing sample' }).fill(EMAIL_SAMPLE);
  await form.getByRole('combobox', { name: 'Kind of app it suits' }).selectOption('email');
  await shot(main, '02-writing-samples-add');
  await form.getByRole('button', { name: 'Add sample' }).click();
  await expectStore(yap, 'writing_samples.json', (s) => s.samples.length === 1 && s.samples[0].kind === 'email' && s.samples[0].text.startsWith('Hiya all,'));
  // Never in config.json.
  expect(JSON.stringify(yap.readJson('config.json'))).not.toContain('allotment committee');

  // A second, any app, added with Ctrl+Enter.
  await group.getByRole('button', { name: 'Add a writing sample' }).click();
  form = group.getByRole('group', { name: 'New writing sample' });
  await form.getByRole('textbox', { name: 'Writing sample' }).fill(CHAT_SAMPLE);
  await main.keyboard.press('Control+Enter');
  await expectStore(yap, 'writing_samples.json', (s) => s.samples.length === 2 && s.samples[1].kind === '');
  const rows = group.locator('[data-sample]');
  await expect(rows).toHaveCount(2);

  // Tagged Chat from its row; edited in place (Esc leaves it as it was).
  await rows.nth(1).getByRole('combobox', { name: 'Kind of app it suits' }).selectOption('chat');
  await expectStore(yap, 'writing_samples.json', (s) => s.samples[1].kind === 'chat');
  await rows.nth(1).getByRole('button', { name: 'Edit writing sample 2' }).click();
  form = group.getByRole('group', { name: 'Edit writing sample' });
  await form.getByRole('textbox', { name: 'Writing sample' }).fill('something else');
  await main.keyboard.press('Escape');
  await expect(rows.nth(1)).toContainText('running a bit late');
  await rows.nth(1).getByRole('button', { name: 'Edit writing sample 2' }).click();
  form = group.getByRole('group', { name: 'Edit writing sample' });
  await form.getByRole('textbox', { name: 'Writing sample' }).fill(`${CHAT_SAMPLE}!`);
  await form.getByRole('button', { name: 'Save' }).click();
  await expectStore(yap, 'writing_samples.json', (s) => s.samples[1].text === `${CHAT_SAMPLE}!` && s.samples[1].kind === 'chat');
  await shot(main, '03-writing-samples-two');

  await expect(group).toContainText('never with AI cleanup');

  await setConfig(yap, main, { theme: 'dark' });
  await group.scrollIntoViewIfNeeded();
  await shot(main, '04-writing-samples-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Deleted, behind the confirm dialog.
  await rows.nth(1).getByRole('button', { name: 'Delete writing sample 2' }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toContainText('Delete this writing sample?');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'writing_samples.json', (s) => s.samples.length === 1 && s.samples[0].kind === 'email');

  // Settings search finds it.
  const search = settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
  await search.fill('writing samples');
  await expect(settingsDialog(main).getByRole('listbox', { name: 'Settings found' }).getByRole('option').filter({ hasText: 'Writing samples' }).first()).toBeVisible();
  await search.fill('');
  await closeSettings(main);
  await yap.invoke('writing_samples_set', { samples: [] });
});

test('a rewrite with samples: at the end of the system prompt, for the kind of app, capped on this PC, named in the receipts', async ({ yap, main, fakeLocal, fakeCloud }) => {
  fakeLocal.reply('transform', (user) => `Polished: ${transformText(user)}`);
  fakeLocal.reply('cleanup', (user) => cleanupTranscript(user));
  await yap.invoke('e2e_target_window', { on: true });
  try {
    // No samples: every request as it was.
    await yap.invoke('writing_samples_set', { samples: [] });
    await yap.invoke('e2e_style_app', { exe: 'OUTLOOK.EXE', title: '' });
    await yap.invoke('e2e_transform', { id: 'polish', selection: 'so um the quiz moved to friday' });
    const plain = lastOf(fakeLocal, 'transform').system;
    expect(plain).toMatch(/^You transform text/);
    expect(plain).not.toContain(BLOCK);

    // Samples for email, chat and any app.
    await yap.invoke('writing_samples_set', {
      samples: [
        { id: '', text: EMAIL_SAMPLE, kind: 'email' },
        { id: '', text: CHAT_SAMPLE, kind: 'chat' },
        { id: '', text: LONG_EMAIL, kind: 'email' },
        { id: '', text: LONG_EMAIL2, kind: 'email' },
        { id: '', text: ANY_SAMPLE, kind: '' },
      ],
    });

    // Polish in an email: the email samples at the end of the system
    // prompt; capped for a model on this PC (the second long one doesn't
    // fit); the user's turn as without.
    let r = await yap.invoke('e2e_transform', { id: 'polish', selection: 'so um the quiz moved to friday' });
    expect(r.outcome).toBe('testMode');
    let req = lastOf(fakeLocal, 'transform');
    expect(req.system.startsWith(`${plain}\n\n${BLOCK}`)).toBe(true);
    expect(req.system).toContain('Quick one – the allotment committee');
    expect(req.system).toContain('the dates work');
    expect(req.system).not.toContain(CHAT_SAMPLE);
    expect(req.system).not.toContain('the venue works');
    expect(req.system.length - plain.length).toBeLessThan(800 + 600);
    expect(transformText(req.user)).toBe('so um the quiz moved to friday');

    // The same again: the same system prompt (what the server caches).
    await yap.invoke('e2e_transform', { id: 'shorter', selection: 'another text altogether' });
    expect(lastOf(fakeLocal, 'transform').system).toBe(req.system);

    // Fix grammar only: never.
    await yap.invoke('e2e_transform', { id: 'grammar', selection: 'so um the quiz moved to friday' });
    expect(lastOf(fakeLocal, 'transform').system).toBe(plain);

    // In a chat app: the chat sample.
    await yap.invoke('e2e_style_app', { exe: 'slack.exe', title: '' });
    await yap.invoke('e2e_transform', { id: 'polish', selection: 'on my way' });
    req = lastOf(fakeLocal, 'transform');
    expect(req.system).toContain(CHAT_SAMPLE);
    expect(req.system).not.toContain('allotment');

    // The rewrite shortcut, writing new text in an email: the samples
    // before the selection's place in its system prompt.
    await yap.invoke('e2e_style_app', { exe: 'OUTLOOK.EXE', title: '' });
    await setConfig(yap, main, { rewriteLastDictation: false });
    const w = await yap.invoke('e2e_rewrite', { text: "tell the team I'll be late" });
    expect(w.outcome).toBe('pasted');
    req = lastOf(fakeLocal, 'rewrite');
    expect(req.system).toContain(BLOCK);
    expect(req.system).toContain('allotment committee');
    expect(req.user).toContain("User's instruction: tell the team I'll be late");

    // An AI prompt: none.
    await yap.invoke('e2e_style_app', { exe: 'ChatGPT.exe', title: '' });
    await yap.invoke('e2e_rewrite', { text: 'make this a list' });
    expect(lastOf(fakeLocal, 'rewrite').system).not.toContain(BLOCK);
    await setConfig(yap, main, { rewriteLastDictation: true });

    // AI cleanup never: its request with samples is exactly the one
    // without, at Medium and at High.
    await yap.invoke('e2e_style_app', { exe: 'OUTLOOK.EXE', title: '' });
    for (const strength of ['medium', 'high']) {
      await setConfig(yap, main, { cleanupStrength: strength });
      await yap.invoke('writing_samples_set', { samples: [] });
      await yap.invoke('e2e_dictation', { text: 'hi all the quiz moved to friday thanks robin' });
      const before = lastOf(fakeLocal, 'cleanup');
      await yap.invoke('writing_samples_set', { samples: [{ id: '', text: EMAIL_SAMPLE, kind: 'email' }] });
      await yap.invoke('e2e_dictation', { text: 'hi all the quiz moved to friday thanks robin' });
      req = lastOf(fakeLocal, 'cleanup');
      expect(req.system).toBe(before.system);
      expect(req.user).toBe(before.user);
      expect(req.system).not.toContain(BLOCK);
    }
    await setConfig(yap, main, { cleanupStrength: 'medium' });

    // A cloud model: more of them fit, and the receipts say they went.
    await yap.invoke('writing_samples_set', {
      samples: [
        { id: '', text: EMAIL_SAMPLE, kind: 'email' },
        { id: '', text: LONG_EMAIL, kind: 'email' },
        { id: '', text: LONG_EMAIL2, kind: 'email' },
      ],
    });
    fakeCloud.reply('transform', (user) => `Polished: ${transformText(user)}`);
    await setConfig(yap, main, { ppProvider: 'groq', ppBaseUrl: fakeCloud.base, ppModel: 'openai/gpt-oss-20b', ppApiKey: GROQ_KEY, ppApiKeys: { groq: GROQ_KEY } });
    await yap.invoke('connections_clear');
    await yap.invoke('e2e_transform', { id: 'polish', selection: 'so um the quiz moved to friday' });
    req = lastOf(fakeCloud, 'transform');
    expect(req.system).toContain('the venue works');
    expect(JSON.stringify(await yap.invoke('connections_summary'))).toContain('Commands + your writing samples · Groq');
    // Grammar: no samples, so no such receipt.
    await yap.invoke('connections_clear');
    await yap.invoke('e2e_transform', { id: 'grammar', selection: 'so um the quiz moved to friday' });
    expect(JSON.stringify(await yap.invoke('connections_summary'))).not.toContain('writing samples');
  } finally {
    await yap.invoke('writing_samples_set', { samples: [] }).catch(() => {});
    await yap.invoke('e2e_style_app', { exe: null, title: null }).catch(() => {});
    await yap.invoke('e2e_target_window', { on: false }).catch(() => {});
    await setConfig(yap, main, { ppProvider: 'custom', ppBaseUrl: fakeLocal.base, ppModel: 'fake-cleanup', ppApiKey: '', cleanupStrength: 'medium', rewriteLastDictation: true }).catch(() => {});
  }
});
