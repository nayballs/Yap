// Chat on your Claude plan (desktop/src-tauri/src/ai/claude_plan.rs, chat/turn/plan.rs,
// desktop/src/lib/settings/ClaudePlanProvider.svelte): Chat's answers from Claude on the
// person's own Claude plan, through their own Claude Code. Here a stand-in plays Claude Code
// (`yap.exe fake-claude <script>`, ai/claude_code/fake.rs; YAP_CLAUDE_EXE and
// YAP_CLAUDE_ARGS_PREFIX), signed in, from support/fake-claude/plan-chat.jsonl: nothing
// goes on the internet and no real Claude Code or account is used.
// - The picker: Claude is one entry with "Your plan | API key" (an Anthropic key saved
//   too); the chip says which pays ("Using Claude plan", "Anthropic API key"); this
//   stand-in refuses get_usage, so no usage ring on the plan (and no error); an API model
//   picked, then back to the plan.
// - Settings → Language Models → Chat → Claude: the plan's card, signed in ("Connected ·
//   Using Claude plan", the version), the switch, Use for Chat.
// - A plan chat: streamed, a web search step ("Searching the web · …"), its chip, the
//   answer's link to the page as Yap's [1] with a Sources row, the byline "Your Claude
//   plan · ≈ $0.0042 on the API"; a second question in the same session (one Claude Code
//   process: claude_sessions.json keeps it, the log says one start); AI usage shows the
//   plan apart from API spend.
// - Claude Code's own copy of a plan chat (its transcript, in its config folder: here a
//   CLAUDE_CONFIG_DIR of the run's own, the stand-in keeping one as Claude Code does)
//   goes when the chat is deleted; another project's in that folder stays.
// - The Voice Assistant on the plan: the answer spoken from its first sentence.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { EXE, RUNS } from './support/yap.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(HERE, 'support', 'fake-claude', 'plan-chat.jsonl');
const ANTHROPIC_KEY = 'sk-ant-e2e-fake-00001234';
const PLAN_CHAT = { enabled: true, provider: 'claude-plan', baseUrl: 'claude-code://plan', model: 'claude-haiku-5-5', apiKey: 'claude-plan', apiKeys: {}, prompt: '', disableThinking: false };
// Claude Code's config folder for this run (never the person's ~/.claude): the stand-in keeps
// each session's transcript in it, as Claude Code does (YAP_FAKE_CLAUDE_TRANSCRIPTS).
const CLAUDE_CONFIG = path.join(RUNS, 'claude-plan-claude-config');
const PROJECTS = path.join(CLAUDE_CONFIG, 'projects');
/** Every transcript the stand-in kept: `{ folder, file }`. */
const transcripts = () =>
  fs.existsSync(PROJECTS)
    ? fs.readdirSync(PROJECTS).flatMap((folder) => fs.readdirSync(path.join(PROJECTS, folder)).filter((f) => f.endsWith('.jsonl')).map((file) => ({ folder, file })))
    : [];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.rmSync(CLAUDE_CONFIG, { recursive: true, force: true });
      fs.mkdirSync(CLAUDE_CONFIG, { recursive: true });
      await use({
        name: 'claude-plan',
        env: { YAP_CLAUDE_EXE: EXE, YAP_CLAUDE_ARGS_PREFIX: JSON.stringify(['fake-claude', SCRIPT]), CLAUDE_CONFIG_DIR: CLAUDE_CONFIG, YAP_FAKE_CLAUDE_TRANSCRIPTS: '1' },
        config: {
          postProcessEnabled: false,
          ppProvider: 'ondevice',
          webSearch: true,
          meetingReminder: 'never',
          // The Voice Assistant reads its answers aloud (test mode keeps what it would say).
          speakReplies: true,
          speakWhen: 'voice',
          ppApiKeys: { 'claude-plan': 'claude-plan', anthropic: ANTHROPIC_KEY },
          llmScopes: { chat: PLAN_CHAT },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const chip = (main) => main.locator('.modelchip .chip');
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const picker = (main) => main.getByRole('dialog', { name: 'Switch the Chat model' });
const pane = (main) => picker(main).locator('.pane');
const railTab = (main, name) => picker(main).getByRole('tab', { name: new RegExp(`^${name}`) });
const spoken = (yap) => yap.invoke('tts_e2e_spoken');
const starts = (yap) => (fs.readFileSync(yap.logPath, 'utf8').match(/Claude plan: a session started for a conversation/g) || []).length;

/** Every line the activity line showed, from now on (chat-activity.spec.js' way). */
async function watchLine(main) {
  await main.evaluate(() => {
    window.__lines = [];
    new MutationObserver(() => {
      const t = document.querySelector('[data-testid="chat-activity"] .aline')?.textContent?.trim();
      if (t && window.__lines.at(-1) !== t) window.__lines.push(t);
    }).observe(document.body, { subtree: true, childList: true, characterData: true });
  });
  return () => main.evaluate(() => window.__lines);
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function newChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}

/** Chat back on the plan's Haiku (as the worker started), whatever a test left. */
async function onPlan(yap, main) {
  await yap.invoke('chat_set_model', { provider: 'claude-plan', model: 'claude-haiku-5-5', baseUrl: null, file: null });
  await openView(main, 'Chat');
  await expect(chip(main)).toContainText('Using Claude plan');
}

test('the picker has one Claude entry, "Your plan | API key", and the chip says which pays', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(chip(main)).toHaveText(/Claude Haiku 5\.5.*· Using Claude plan/);
  // The plan's usage ring is where Manage usage was (claude-plan-usage.spec.js); this
  // stand-in refuses get_usage, as an older Claude Code would: no ring at all, no error.
  await expect.poll(() => fs.readFileSync(yap.logPath, 'utf8'), { timeout: 20_000 }).toMatch(/Claude plan: usage asked.*outcome="?refused/);
  await expect(main.getByTestId('plan-usage-ring')).toHaveCount(0);
  await expect(main.locator('.modelchip .usage')).toHaveCount(0);
  await chip(main).click();
  await expect(picker(main)).toBeVisible();
  // Opened on Claude, the plan's way: both connected, so the switch.
  await expect(railTab(main, 'Claude')).toHaveAttribute('aria-selected', 'true');
  const ways = pane(main).getByTestId('claude-ways');
  await expect(ways.getByRole('tab')).toHaveText(['Your plan', 'API key']);
  await expect(ways.getByRole('tab', { name: 'Your plan' })).toHaveAttribute('aria-selected', 'true');
  await expect(pane(main).locator('.phead')).toContainText('Claude');
  await expect(pane(main).locator('.phead')).toContainText('Using Claude plan');
  await expect(pane(main).locator('.mrow .nm')).toHaveText(['Claude Opus 5.5', 'Claude Fable 5.1', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
  await expect(pane(main).getByRole('button', { name: /^Claude Haiku 5\.5/ })).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '01-claude-picker-your-plan');

  // The API key's way: Anthropic's models, its key masked.
  await ways.getByRole('tab', { name: 'API key' }).click();
  await expect(pane(main).locator('.phead')).toContainText('Anthropic');
  await expect(pane(main).locator('.phead .keymask')).toHaveText('sk-…1234');
  await shot(main, '02-claude-picker-api-key');
  await pane(main).getByRole('button', { name: /^Claude Sonnet 5\.5/ }).click();
  await expect(picker(main)).toBeHidden();
  await expect(chip(main)).toHaveText(/Claude Sonnet 5\.5.*· Anthropic API key/);
  await expect(main.locator('.modelchip .usage')).toHaveCount(0);
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'anthropic' && c.llmScopes.chat.model === 'claude-sonnet-5-5');

  // Opened again: on the key's way; back to the plan.
  await chip(main).click();
  await expect(pane(main).getByTestId('claude-ways').getByRole('tab', { name: 'API key' })).toHaveAttribute('aria-selected', 'true');
  await pane(main).getByTestId('claude-ways').getByRole('tab', { name: 'Your plan' }).click();
  await pane(main).getByRole('button', { name: /^Claude Haiku 5\.5/ }).click();
  await expect(chip(main)).toHaveText(/Claude Haiku 5\.5.*· Using Claude plan/);
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'claude-plan' && c.llmScopes.chat.model === 'claude-haiku-5-5');
});

test('Settings → Chat → Claude: the plan card signed in, the switch and Use for Chat', async ({ yap, main, shot }) => {
  await onPlan(yap, main);
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Chat & Voice Assistant' }).click();
  await dialog.getByRole('tab', { name: 'Claude' }).click();
  const card = dialog.getByTestId('claude-plan-card');
  await expect(card).toContainText("Yap runs Claude Code on this PC, signed in with your own Claude account. Answers use your plan's limits.");
  await expect(card.locator('[data-state="ready"]')).toContainText('Connected · Using Claude plan');
  await expect(card).toContainText('Claude Code 2.1.296');
  await expect(card.getByRole('link', { name: 'Manage usage' })).toBeVisible();
  await expect(card.getByTestId('claude-plan-copies')).toHaveText('Claude Code keeps its own copy of each plan chat; Yap deletes it with the chat.');
  await expect(dialog.getByTestId('claude-provider')).toHaveAttribute('data-way', 'claude-plan');
  await shot(main, '03-settings-claude-plan-connected');

  // The API key's way, then Use for Chat on the plan's card.
  const ways = dialog.locator('[data-setting="claude-ways"]');
  await ways.getByRole('tab', { name: 'API key' }).click();
  await expect(dialog.getByTestId('claude-provider')).toHaveAttribute('data-way', 'anthropic');
  await expect(dialog.getByLabel('Anthropic API key')).toHaveCount(0); // masked: the saved key
  await ways.getByRole('tab', { name: 'Your plan' }).click();
  await expect(dialog.getByTestId('claude-provider')).toHaveAttribute('data-way', 'claude-plan');
  await expect(card.getByRole('button', { name: 'Use for Chat' })).toHaveCount(0);
  await closeSettings(main);
  await expectStore(yap, 'config.json', (c) => c.llmScopes.chat.provider === 'claude-plan');
});

test('a plan chat streams, searches the web, cites its page and keeps one session for the conversation', async ({ yap, main, shot }) => {
  await onPlan(yap, main);
  await newChat(main);
  const before = starts(yap);
  const lines = await watchLine(main);
  await send(main, 'When are the tides at Example Bay today?');
  // What it's doing while it works: the search, its query whole.
  await expect.poll(lines, { timeout: 15_000 }).toContain('Searching the web · tide times example bay');
  await shot(main, '04-plan-chat-searching');
  const answer = thread(main).locator('.cbubble.assistant').last();
  await expect(answer).toContainText('High tide is at 06:12 and low tide at 12:30', { timeout: 15_000 });
  await expect(answer).toContainText('Take a coat.');
  // The link to the page became Yap's own [1]: a pill, and the Sources row.
  await expect(answer).not.toContainText('https://tides.example.com');
  const sources = thread(main).getByTestId('chat-sources').last();
  await expect(sources.getByTestId('chat-source')).toHaveCount(1);
  await expect(sources).toContainText('tides.example.com');
  // How it was paid: the plan, with what it would have cost on the API.
  await expect(thread(main).locator('.byline').last()).toHaveText('Claude Haiku 5.5 · Your Claude plan · ≈ $0.0042 on the API');
  await shot(main, '05-plan-chat-answered');
  expect(starts(yap)).toBe(before + 1);
  const conv = () => yap.readJson('chats.json').conversations.at(-1);
  await expect.poll(() => conv().messages.length).toBe(2);
  const saved = conv().messages[1];
  expect(saved.model).toMatchObject({ provider: 'claude-plan', id: 'claude-haiku-5-5' });
  expect(saved.text).toContain('[1]');
  expect(saved.sources.map((s) => s.url)).toEqual(['https://tides.example.com/bay']);
  expect(saved.tools).toEqual(['Searched the web: "tide times example bay"']);
  expect(saved.tokens.costMicros).toBe(4200);
  // Its session kept for the conversation, with what it has seen.
  const kept = () => yap.readJson('claude_sessions.json').conversations[String(conv().id)];
  await expect.poll(() => kept()?.seen).toBe(2);
  const session = kept().session;
  expect(session).toMatch(/^[0-9a-f-]{36}$/);

  // A second question: the same warm process, the same session.
  await send(main, 'And when is it best for a walk?');
  await expect(thread(main).locator('.cbubble.assistant').last()).toContainText('Same session, second answer', { timeout: 15_000 });
  await expect.poll(() => kept()?.seen).toBe(4);
  expect(kept().session).toBe(session);
  expect(starts(yap)).toBe(before + 1);
  await shot(main, '06-plan-chat-second-turn');

  // AI usage: the plan's answers apart from API spend, with their API value.
  await openView(main, 'Insights');
  const plan = main.getByTestId('ai-usage-plan');
  await expect(plan).toContainText('on your Claude plan', { timeout: 10_000 });
  await expect(plan).toContainText('on the API, not charged');
  await expect(main.getByTestId('ai-usage-models')).toContainText('on your Claude plan · ≈ $0.0053 on the API');
  await shot(main, '07-ai-usage-claude-plan');

  // Claude Code's own copy of the chat: its session's transcript, filed under Yap's
  // claude/chat folder (the run's data folder, encoded as Claude Code names it).
  const convId = conv().id;
  const mine = transcripts().filter((t) => t.file === `${session}.jsonl`);
  expect(mine.map((t) => t.folder.toLowerCase())).toEqual([path.join(yap.dataDir, 'claude', 'chat').replace(/[^a-zA-Z0-9]/g, '-').toLowerCase()]);
  // Another project's in the same config folder (the person's own job, say), with the same
  // file name even: never touched.
  const theirs = path.join(PROJECTS, 'E--Projects-garden-planner', `${session}.jsonl`);
  fs.mkdirSync(path.dirname(theirs), { recursive: true });
  fs.writeFileSync(theirs, '{}\n');
  // The chat deleted: Claude Code's copy goes with it.
  await yap.invoke('chat_delete', { id: convId });
  await expect.poll(() => transcripts().filter((t) => t.file === `${session}.jsonl`).map((t) => t.folder)).toEqual(['E--Projects-garden-planner']);
  expect(fs.existsSync(theirs)).toBe(true);
  await expect.poll(() => yap.readJson('claude_sessions.json').conversations[String(convId)]).toBeUndefined();
});

test('the Voice Assistant on the plan speaks the answer from its first sentence', async ({ yap, main }) => {
  await onPlan(yap, main);
  await newChat(main);
  await yap.invoke('tts_e2e_spoken', { clear: true });
  await yap.invoke('e2e_assistant_ask', { text: 'what are the tides today', listenMs: 0 });
  await expect
    .poll(async () => (await spoken(yap)).map((s) => s.text).join(' '), { timeout: 20_000 })
    .toContain('High tide is at six twelve and low tide at twelve thirty.');
  const said = (await spoken(yap)).map((s) => s.text).join(' ');
  // Never the words before the search, a citation's number or an address.
  expect(said).not.toContain('Let me look that up');
  expect(said).not.toContain('thirty one');
  expect(said).not.toContain('https');
  await expect.poll(async () => (await spoken(yap)).map((s) => s.text).join(' '), { timeout: 10_000 }).toContain('Take a coat.');
  await yap.invoke('tts_stop');
});
