// Sign in with ChatGPT (src-tauri/src/ai/chatgpt.rs, ai/llm/responses.rs,
// src/lib/settings/ChatGptSignIn.svelte): Yap's AI on the person's own
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
// - Sign out revokes the grant at OpenAI.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
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

test('Sign out revokes the grant at OpenAI and leaves the registration for next time', async ({ yap, main, gpt }) => {
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
  await closeSettings(main);
});
