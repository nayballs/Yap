// Sign in with ChatGPT (desktop/src-tauri/src/ai/chatgpt.rs, ai/llm/responses.rs,
// desktop/src/lib/settings/ChatGptSignIn.svelte): Yap's AI on the person's own
// ChatGPT plan, against a local fake of OpenAI's sign-in and Responses API
// (support/fake-chatgpt.js). Nothing goes on the internet.
// - Settings → Language Models → Chat → Cloud providers → ChatGPT: no key
//   field, "Continue with ChatGPT"; a decline says so; the consent page
//   allowed, Yap registers itself (`dynamic_agent_client`, "Yap", a host id),
//   swaps the code with its PKCE verifier and checks the signed ID token;
//   "Connected · Using ChatGPT plan" with Manage usage; the one-time
//   "You're using your ChatGPT plan" notice; the account's own models;
// - tokens stay out of config.json and chatgpt.json (the key store holds
//   only a marker);
// - Chat on the plan: "Using ChatGPT plan" by the model; a streamed answer;
//   a namespaced tool call whose reasoning goes back whole with its
//   result; the plan's usage limit said with Manage usage;
// - one ChatGPT entry for OpenAI's two ways (2026-11-02): the plan alone in
//   Chat's picker (its models, "Use an OpenAI API key instead…"), a key
//   added there ("Your plan | API key", the key's list), an API model picked
//   ("OpenAI API key" on the chip), opened again on the way in use, back to
//   the plan; Recent and search say which way; Settings' ChatGPT tab with
//   both, its switch moving the scope between "chatgpt" and "openai";
// - Sign out revokes the grant at OpenAI; then Settings' tab with a key only
//   ("Use your ChatGPT plan instead…" signs in), and with nothing connected
//   ("Use an API key instead" opens the key field).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore, setConfig } from './support/fixtures.js';
import { startFakeChatGpt } from './support/fake-chatgpt.js';

const CHAT = { enabled: true, provider: 'chatgpt', baseUrl: 'https://api.openai.com/v1', model: 'gpt-6.1-sol', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };

const test = base.extend({
  gpt: [
    async ({}, use) => {
      const fake = await startFakeChatGpt();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ gpt }, use) => {
      await use({
        name: 'chatgpt',
        env: gpt.env,
        config: { postProcessEnabled: false, ppProvider: 'ondevice', llmScopes: { chat: CHAT }, meetingReminder: 'never' },
      });
    },
    { scope: 'worker' },
  ],
});

const OPENAI_KEY = 'sk-proj-e2e-fake-0000007890';
const opened = (yap) => yap.invoke('chatgpt_e2e_opened');
const raw = (yap, file) => {
  try {
    return fs.readFileSync(path.join(yap.dataDir, file), 'utf8');
  } catch {
    return '';
  }
};
const chip = (main) => main.locator('.modelchip .chip');
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const pane = (main) => picker(main).locator('.pane');
const railTab = (main, name) => picker(main).getByRole('tab', { name: new RegExp(`^${name}`) });

async function chatgptTab(main) {
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  await dialog.getByRole('tab', { name: 'ChatGPT' }).click();
  return dialog;
}

async function send(main, text) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('Continue with ChatGPT: declined, then signed in on the plan, its models listed, no token in Yap\'s files', async ({ yap, main, gpt, shot }) => {
  const dialog = await chatgptTab(main);
  const card = dialog.getByTestId('chatgpt-card');
  await expect(card.getByRole('button', { name: 'Continue with ChatGPT' })).toBeVisible();
  await expect(dialog.getByPlaceholder('sk-…')).toHaveCount(0);
  await shot(main, '01-chatgpt-signed-out');

  // Declined on the consent page: said, nothing connected.
  await card.getByRole('button', { name: 'Continue with ChatGPT' }).click();
  await expect(card).toContainText('Finish signing in in your browser');
  const denied = await fetch(gpt.deny((await opened(yap)).at(-1)));
  expect(await denied.text()).toContain('Not connected');
  await expect(card).toContainText("You didn't allow Yap to use ChatGPT.");

  // Allowed: a new registration, PKCE, the loopback listener at /auth/callback.
  // OpenAI refuses that first code (`invalid_grant`, as on the first real
  // sign-in, 2 Nov 2026): Yap authorizes again once, with its issued id.
  gpt.refuseNewRegistrationCodes(true);
  await card.getByRole('button', { name: 'Continue with ChatGPT' }).click();
  const authUrl = (await opened(yap)).at(-1);
  const tabs = (await opened(yap)).length;
  expect(authUrl.startsWith(`${gpt.base}/api/accounts/authorize`)).toBe(true);
  const q = new URL(authUrl).searchParams;
  expect(q.get('client_id')).toBe('dynamic_agent_client');
  expect(q.get('agent_name_hint')).toBe('Yap');
  expect(q.get('ext_agent_host_id')).toMatch(/^urn:uuid:[0-9a-f-]{36}$/);
  expect(q.get('scope').split(' ')).toContain('chatgpt.tokens.use.direct');
  expect(q.get('resource')).toBe('https://api.openai.com/v1');
  expect(q.get('code_challenge_method')).toBe('S256');
  expect(q.get('redirect_uri')).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/auth\/callback$/);
  const refused = await fetch(gpt.approve(authUrl));
  expect(await refused.text()).toContain('One more step');
  await expect.poll(async () => (await opened(yap)).length).toBe(tabs + 1);
  const retry = new URL((await opened(yap)).at(-1)).searchParams;
  expect(retry.get('client_id')).toBe(gpt.issued);
  expect(retry.get('agent_name_hint')).toBeNull();
  expect(retry.get('ext_agent_host_id')).toBe(q.get('ext_agent_host_id'));
  await expect(card).toContainText('Finish signing in in your browser');
  const back = await fetch(gpt.approve((await opened(yap)).at(-1)));
  gpt.refuseNewRegistrationCodes(false);
  expect(back.status).toBe(200);
  expect(await back.text()).toContain('ChatGPT connected');

  // The one-time notice, in OpenAI's words.
  const notice = main.getByRole('dialog', { name: "You're using your ChatGPT plan" });
  await expect(notice).toBeVisible();
  await shot(main, '02-chatgpt-notice');
  await notice.getByRole('button', { name: 'Got it' }).click();
  await expect(notice).toBeHidden();
  await expect(card).toContainText('Connected');
  await expect(card).toContainText('Using ChatGPT plan');
  await expect(card).toContainText('sam@example.com');
  await expect(card.getByRole('link', { name: 'Manage usage' })).toBeVisible();
  await shot(main, '03-chatgpt-connected');

  // The account's own models, in its order (a hidden one left out).
  await expect(dialog.getByText('GPT-6.1 Sol').first()).toBeVisible();
  await expect(dialog.getByText('Hidden')).toHaveCount(0);

  // The registration and the host id are kept; no token is in Yap's files.
  await expectStore(yap, 'chatgpt.json', (a) => a.clientId === gpt.issued && a.signedIn && a.noticeSeen && a.hostId.startsWith('urn:uuid:'));
  await expectStore(yap, 'config.json', (c) => c.ppApiKeys?.chatgpt === 'chatgpt-plan' && c.llmScopes?.chat?.apiKey === 'chatgpt-plan');
  for (const file of ['config.json', 'chatgpt.json']) expect(raw(yap, file)).not.toMatch(/e2e-(access|refresh)/);
  expect(raw(yap, 'chatgpt-secret.e2e.json')).toMatch(/e2e-refresh/);
  await closeSettings(main);
  expect(gpt.problems).toEqual([]);
});

test('Chat on the plan: streamed, a namespaced tool step with its reasoning sent back, the usage limit said', async ({ main, gpt, shot }) => {
  await openView(main, 'Chat');
  await expect(chip(main)).toContainText('GPT-6.1 Sol');
  await expect(chip(main)).toContainText('Using ChatGPT plan');
  await expect(main.locator('.modelchip').getByRole('link', { name: 'Manage usage' })).toBeVisible();

  await send(main, 'Say hello');
  await expect(thread(main)).toContainText('Hello from your ChatGPT plan.');
  const first = gpt.responses.at(-1);
  expect(first.model).toBe('gpt-6.1-sol');
  expect(first.instructions).toBeTruthy();
  await shot(main, '04-chatgpt-chat');

  // A tool step: search_notes called in Yap's namespace, then answered.
  const before = gpt.responses.length;
  await send(main, 'What do my notes say about the launch?');
  await expect(thread(main)).toContainText('I looked in your notes.');
  expect(gpt.responses.length).toBeGreaterThanOrEqual(before + 2);
  expect(gpt.responses[before].tools?.[0]).toMatchObject({ type: 'namespace', name: 'yap' });

  // The plan's usage limit: said, with where to manage it.
  await send(main, 'Go over my limit');
  // (Chat's error line, under the question: "Couldn't send …".)
  const limit = main.getByText("You've reached your ChatGPT plan's usage limit", { exact: false });
  await expect(limit).toBeVisible();
  await expect(limit).toContainText('chatgpt.com/settings/usage');
  await shot(main, '05-chatgpt-limit');
  expect(gpt.problems).toEqual([]);
});

test('one ChatGPT entry: the plan alone, then with an OpenAI key and "Your plan | API key"', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await chip(main).click();
  const pop = picker(main);
  await expect(pop).toBeVisible();

  // On the plan: the ChatGPT entry (no OpenAI one), the account's models.
  await expect(railTab(main, 'ChatGPT')).toHaveAttribute('aria-selected', 'true');
  await expect(railTab(main, 'ChatGPT')).toHaveText('ChatGPT, connected');
  await expect(pop.getByRole('tab', { name: /^OpenAI/ })).toHaveCount(0);
  const head = pop.locator('.phead');
  await expect(head.locator('.ptitle')).toHaveText('ChatGPT');
  await expect(head).toContainText('Using ChatGPT plan');
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['GPT-6.1 Sol', 'GPT-6 Luna']);
  await expect(pane(main).getByRole('button', { name: /^GPT-6\.1 Sol/ })).toHaveAttribute('aria-pressed', 'true');
  await expect(pane(main).getByTestId('gpt-ways')).toHaveCount(0);
  const other = pane(main).getByRole('button', { name: 'Use an OpenAI API key instead…' });
  await expect(other).toBeVisible();
  await shot(main, '06-picker-plan-only');

  // An OpenAI key too, from that link: both ways, the key's list on show.
  await other.click();
  const form = pane(main).locator('form.connect');
  await expect(form).toContainText('Use an OpenAI API key');
  await expect(form.getByLabel('OpenAI API key')).toBeFocused();
  await form.getByLabel('OpenAI API key').fill(OPENAI_KEY);
  await form.getByRole('button', { name: 'Save key' }).click();
  await expectStore(yap, 'config.json', (c) => c.ppApiKeys?.openai === OPENAI_KEY);
  const ways = pane(main).getByTestId('gpt-ways');
  await expect(ways.getByRole('tab')).toHaveText(['Your plan', 'API key']);
  await expect(ways.getByRole('tab', { name: 'API key' })).toHaveAttribute('aria-selected', 'true');
  await expect(head.locator('.keymask')).toHaveText('sk-…7890');
  await expect(pane(main).getByRole('group', { name: 'Latest' }).locator('.nm')).toHaveText(['GPT-6 Astra', 'GPT-6 Luna']);
  await expect(pane(main).getByRole('button', { name: /instead…$/ })).toHaveCount(0);
  await shot(main, '07-picker-both-api-key');
  // The plan's list, a click away.
  await ways.getByRole('tab', { name: 'Your plan' }).click();
  await expect(head).toContainText('Using ChatGPT plan');
  await expect(head.locator('.keymask')).toHaveCount(0);
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['GPT-6.1 Sol', 'GPT-6 Luna']);
  await shot(main, '08-picker-both-plan');

  // An API model picked: provider "openai", and the chip says the key pays.
  await ways.getByRole('tab', { name: 'API key' }).click();
  await pane(main).getByRole('group', { name: 'Latest' }).getByRole('button', { name: /^GPT-6 Astra/ }).click();
  await expect(pop).toBeHidden();
  await expect(chip(main)).toContainText('GPT-6 Astra');
  await expect(chip(main)).toContainText('· OpenAI API key');
  await expect(main.locator('.modelchip').getByRole('link', { name: 'Manage usage' })).toHaveCount(0);
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'openai' && c.llmScopes.chat.model === 'gpt-6-astra' && c.llmScopes.chat.apiKey === OPENAI_KEY);

  // Opened again: the switch on the way the model in use goes; back to the plan.
  await chip(main).click();
  await expect(ways.getByRole('tab', { name: 'API key' })).toHaveAttribute('aria-selected', 'true');
  await ways.getByRole('tab', { name: 'Your plan' }).click();
  await pane(main).getByRole('button', { name: /^GPT-6\.1 Sol/ }).click();
  await expect(pop).toBeHidden();
  await expect(chip(main)).toContainText('Using ChatGPT plan');
  await expect(main.locator('.modelchip').getByRole('link', { name: 'Manage usage' })).toBeVisible();
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'chatgpt' && c.llmScopes.chat.model === 'gpt-6.1-sol');
  await chip(main).click();
  await expect(ways.getByRole('tab', { name: 'Your plan' })).toHaveAttribute('aria-selected', 'true');

  // Recent and search: each row says which way pays.
  await railTab(main, 'Recent').click();
  await expect(pane(main).locator('.mrow').filter({ hasText: 'GPT-6.1 Sol' })).toContainText('ChatGPT plan');
  await expect(pane(main).locator('.mrow').filter({ hasText: 'GPT-6 Astra' })).toContainText('OpenAI API key');
  await pop.getByRole('searchbox', { name: 'Search models and providers' }).fill('gpt-6 luna');
  await expect(pane(main).getByRole('group').locator('.gh')).toHaveText(['ChatGPT plan', 'OpenAI API key']);
  await main.keyboard.press('Escape');
  await expect(pop).toBeHidden();

  // Settings' ChatGPT tab: the same switch, moving the scope between the two.
  const dialog = await chatgptTab(main);
  const gptTab = dialog.getByTestId('chatgpt-provider');
  const sways = gptTab.getByRole('tablist', { name: 'Connect ChatGPT with' });
  await expect(sways.getByRole('tab', { name: 'Your plan' })).toHaveAttribute('aria-selected', 'true');
  await expect(gptTab.getByTestId('chatgpt-card')).toContainText('Using ChatGPT plan');
  await expect(gptTab.getByRole('button', { name: /instead/ })).toHaveCount(0);
  await shot(main, '09-settings-both-plan');
  await sways.getByRole('tab', { name: 'API key' }).click();
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'openai' && c.llmScopes.chat.apiKey === OPENAI_KEY);
  await expect(gptTab.getByTestId('chatgpt-card')).toHaveCount(0);
  await expect(gptTab.locator('.keymask')).toContainText('sk-…7890');
  await shot(main, '10-settings-both-api-key');
  await sways.getByRole('tab', { name: 'Your plan' }).click();
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'chatgpt' && c.llmScopes.chat.apiKey === 'chatgpt-plan');
  await expect(gptTab.getByTestId('chatgpt-card')).toContainText('Using ChatGPT plan');
  await closeSettings(main);
});

test('Sign out revokes the grant at OpenAI and leaves the registration for next time', async ({ yap, main, gpt, shot }) => {
  const dialog = await chatgptTab(main);
  const card = dialog.getByTestId('chatgpt-card');
  await card.getByRole('button', { name: 'Sign out' }).click();
  await expect(card.getByRole('button', { name: 'Continue with ChatGPT' })).toBeVisible();
  expect(gpt.revoked.at(-1)).toMatchObject({ token_type_hint: 'refresh_token', client_id: gpt.issued });
  await expectStore(yap, 'chatgpt.json', (a) => !a.signedIn && a.clientId === gpt.issued);
  await expectStore(yap, 'config.json', (c) => !c.ppApiKeys?.chatgpt);
  expect(raw(yap, 'chatgpt-secret.e2e.json')).not.toMatch(/e2e-refresh/);

  // Signing in again reuses Yap's registration (no new one).
  await card.getByRole('button', { name: 'Continue with ChatGPT' }).click();
  const again = new URL((await opened(yap)).at(-1)).searchParams;
  expect(again.get('client_id')).toBe(gpt.issued);
  expect(again.get('agent_name_hint')).toBeNull();
  await card.getByRole('button', { name: 'Cancel' }).click();
  await expect(card.getByRole('button', { name: 'Continue with ChatGPT' })).toBeVisible();

  // An OpenAI key only (the test before saved one): its way, a click away.
  if (!yap.readJson('config.json').ppApiKeys?.openai) {
    await closeSettings(main);
    await setConfig(yap, main, { ppApiKeys: { ...yap.readJson('config.json').ppApiKeys, openai: OPENAI_KEY } });
    await chatgptTab(main);
  }
  const gptTab = dialog.getByTestId('chatgpt-provider');
  const toggle = gptTab.getByRole('button', { name: 'Use an API key instead' });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'openai' && c.llmScopes.chat.apiKey === OPENAI_KEY);
  await expect(gptTab.getByTestId('chatgpt-card')).toHaveCount(0);
  await expect(gptTab.locator('.keymask')).toContainText('sk-…7890');
  await expect(gptTab.getByRole('heading', { name: 'Model' })).toBeVisible();
  await gptTab.getByRole('button', { name: 'Use your ChatGPT plan instead…' }).scrollIntoViewIfNeeded();
  await shot(main, '11-settings-key-only');
  // "Use your ChatGPT plan instead…": back on the plan, signing in.
  const tabs = (await opened(yap)).length;
  await gptTab.getByRole('button', { name: 'Use your ChatGPT plan instead…' }).click();
  await expect.poll(async () => (await opened(yap)).length).toBe(tabs + 1);
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'chatgpt');
  await expect(gptTab.getByTestId('chatgpt-card')).toContainText('Finish signing in in your browser');
  await gptTab.getByTestId('chatgpt-card').getByRole('button', { name: 'Cancel' }).click();
  await closeSettings(main);

  // Nothing connected: the plan's card, the key's field only when asked for.
  const { openai: _drop, ...keys } = yap.readJson('config.json').ppApiKeys || {};
  await setConfig(yap, main, { ppApiKeys: keys });
  const again2 = await chatgptTab(main);
  const none = again2.getByTestId('chatgpt-provider');
  await expect(none.getByRole('button', { name: 'Continue with ChatGPT' })).toBeVisible();
  await expect(none.getByPlaceholder('sk-…')).toHaveCount(0);
  await expect(none.getByRole('heading', { name: 'Model' })).toHaveCount(0);
  await shot(main, '12-settings-nothing-connected');
  await none.getByRole('button', { name: 'Use an API key instead' }).click();
  await expect(none.getByRole('button', { name: 'Use an API key instead' })).toHaveAttribute('aria-expanded', 'true');
  await expect(none.getByPlaceholder('sk-…')).toBeVisible();
  await expect(none.getByRole('button', { name: 'Continue with ChatGPT' })).toBeVisible();
  await none.getByPlaceholder('sk-…').scrollIntoViewIfNeeded();
  await shot(main, '13-settings-nothing-connected-key');
  await none.getByRole('button', { name: 'Use an API key instead' }).click();
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'chatgpt');
  await expect(none.getByPlaceholder('sk-…')).toHaveCount(0);
  await closeSettings(main);
});
