// Your Claude plan's usage limits (ai/claude_code/usage.rs, ai/claude_plan/usage.rs,
// views/chat/PlanUsageRing.svelte, components/PlanUsageRows.svelte): on the plan, a small ring
// beside the model chip in place of "Manage usage", filled to the highest active limit and
// coloured by its severity; "Plan usage limits" on hover or click; Settings' plan card the same
// rows. The numbers are Claude Code's own `get_usage` answer, played here by the stand-in
// (`yap.exe fake-claude`, its `_usage`) from Yap's synthetic recording
// (ai/claude_code/fixtures/2.1.296/synthetic-get-usage.json) with its times moved to now: no
// real Claude Code, account or numbers.
// - The ring at a normal percent (a one-off probe when the composer mounts: no session warm
//   yet), its popover's rows, reset times in local time (only the time today), extra usage in
//   the answer's currency, See all → the plan's usage page.
// - After a plan turn: how full the chat is (its last model call's tokens, not the turn's sum),
//   Yap's context ring not shown on the plan; a turn's rate-limit event moving the ring live
//   (86 %, a warning), then the answer after the turn (88 %); light and dark.
// - Settings → Language Models → Chat → Claude: the card's rows under Connected.
// - The API key: no plan ring, Yap's own context ring.
// (No ring when Claude Code refuses `get_usage`: claude-plan.spec.js.)
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { RECORDINGS, fakeClaude, planConfig, messageBox, claudeTab } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';

const HERE = path.join(RUNS, 'claude-plan-usage-spec');
const SCRIPT = path.join(HERE, 'plan-usage.jsonl');
const SID = '{{session_id}}';
const MODEL = 'claude-haiku-5-5';
const ANTHROPIC_KEY = 'sk-ant-e2e-fake-00004321';

// ---- when the limits reset: the session later today, the week in three days ----

const now = Date.now();
const MINUTE = 60_000;
const endOfToday = new Date(new Date(now).setHours(23, 59, 0, 0)).getTime();
/** Whole minutes, the session still today (at 11:58 pm: a minute on). */
const SESSION_AT = Math.floor(Math.min(now + 2 * 3_600_000, endOfToday) / MINUTE) * MINUTE;
const WEEK_AT = Math.floor((now + 3 * 86_400_000) / MINUTE) * MINUTE;
const iso = (ms) => new Date(ms).toISOString();

// ---- the stand-in's get_usage answers: the recording's, its times moved ----

const RECORDED = JSON.parse(fs.readFileSync(path.join(RECORDINGS, 'synthetic-get-usage.json'), 'utf8')).response.response;
function answer(weekly, severity = 'normal') {
  const body = structuredClone(RECORDED);
  const rl = body.rate_limits;
  rl.five_hour.resets_at = iso(SESSION_AT);
  rl.seven_day = { utilization: weekly, resets_at: iso(WEEK_AT) };
  rl.model_scoped[0].resets_at = iso(WEEK_AT);
  for (const l of rl.limits) {
    if (l.resets_at) l.resets_at = iso(WEEK_AT);
    if (l.kind === 'weekly_all') Object.assign(l, { percent: weekly, severity });
  }
  return body;
}
const NORMAL = answer(9);
const WARNING = answer(88, 'warning');

// ---- the plan chat's turns ----

const ev = (event) => ({ type: 'stream_event', event, parent_tool_use_id: null, session_id: SID });
const INIT = {
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: SID,
  tools: ['WebFetch', 'WebSearch'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: MODEL,
  permissionMode: 'dontAsk',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
};
/** One answer: the call's prompt (from the cache mostly) and its 40 words' tokens. */
const said = (id, text, prompt, ...beforeEnd) => [
  ev({ type: 'message_start', message: { id, type: 'message', role: 'assistant', model: MODEL, content: [], usage: { input_tokens: 60, cache_read_input_tokens: prompt - 60, output_tokens: 1 } } }),
  ev({ type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }),
  ev({ type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }),
  ...beforeEnd,
  ev({ type: 'content_block_stop', index: 0 }),
  ev({ type: 'message_delta', delta: { stop_reason: 'end_turn' }, usage: { output_tokens: 40 } }),
  { type: 'assistant', message: { id, type: 'message', role: 'assistant', model: MODEL, content: [{ type: 'text', text }], stop_reason: 'end_turn' }, parent_tool_use_id: null, session_id: SID },
  ev({ type: 'message_stop' }),
  {
    type: 'result',
    subtype: 'success',
    is_error: false,
    num_turns: 1,
    result: text,
    session_id: SID,
    total_cost_usd: 0.002,
    // The turn's sum (were it the conversation's, 41 %): the ring never shows it.
    usage: { input_tokens: 120, cache_read_input_tokens: 81_880, cache_creation_input_tokens: 0, output_tokens: 80 },
    modelUsage: { [MODEL]: { inputTokens: 120, outputTokens: 80, cacheReadInputTokens: 81_880, cacheCreationInputTokens: 0, costUSD: 0.002, contextWindow: 200_000, maxOutputTokens: 64_000 } },
    permission_denials: [],
    terminal_reason: 'completed',
  },
];
const LINES = [
  { _usage: NORMAL },
  { _wait_user: true },
  INIT,
  // 32,000 read, 40 written: 16 % of 200K.
  ...said('msg_u1', 'First answer on the plan.', 32_000),
  { _wait_user: true },
  INIT,
  // The week's limit moves mid-turn (86 %, a warning); after the turn Claude Code says 88 %.
  { type: 'rate_limit_event', rate_limit_info: { status: 'allowed_warning', rateLimitType: 'seven_day', utilization: 0.86, resetsAt: Math.floor(WEEK_AT / 1000) }, session_id: SID },
  ...said('msg_u2', 'Second answer on the plan.', 33_960, { _sleep_ms: 4000 }, { _usage: WARNING }),
  { _wait_user: true },
];

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      fs.mkdirSync(HERE, { recursive: true });
      fs.writeFileSync(SCRIPT, LINES.map((l) => JSON.stringify(l)).join('\n') + '\n');
      await use({
        name: 'claude-plan-usage',
        env: fakeClaude(SCRIPT),
        config: planConfig({ 'claude-plan': 'claude-plan', anthropic: ANTHROPIC_KEY }, { memoryLearnChats: false }),
      });
    },
    { scope: 'worker' },
  ],
});

const ring = (main) => main.getByTestId('plan-usage-ring');
const popover = (main) => main.getByRole('dialog', { name: 'Plan usage limits' });
const chip = (main) => main.locator('.modelchip .chip');
const lastAnswer = (main) => main.locator('.msgs .cbubble.assistant').last();
/** The pointer off the ring and its popover (which sits over the message box): up in the chat. */
const away = (main) => main.mouse.move(700, 120);

/** What the page shows for these times and amounts, in the WebView's own locale. */
function expected(main) {
  return main.evaluate(
    ({ session, week }) => {
      const time = (ms) => new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
      const day = (ms) => new Date(ms).toLocaleDateString(undefined, { weekday: 'short' });
      const eur = (n) => new Intl.NumberFormat(undefined, { style: 'currency', currency: 'EUR' }).format(n);
      return { session: `Resets ${time(session)}`, week: `Resets ${day(week)} ${time(week)}`, extra: `Extra usage · ${eur(3.4)} of ${eur(25)}` };
    },
    { session: SESSION_AT, week: WEEK_AT }
  );
}

async function pickTheme(main, yap, label) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  await group.getByRole('tablist', { name: 'Theme' }).getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase(), `the theme ${label} to be saved`);
  await closeSettings(main);
}

async function send(main, text) {
  await messageBox(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('the ring at a normal percent: its popover, reset times, extra usage and See all', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(chip(main)).toContainText('Using Claude plan');
  // No session is warm yet: a one-off probe, the first time the composer shows the plan.
  await expect(ring(main)).toHaveAttribute('data-percent', '23', { timeout: 20_000 });
  await expect(ring(main)).toHaveAttribute('data-severity', 'normal');
  await expect(main.locator('.modelchip .usage')).toHaveCount(0); // "Manage usage" is the ring now
  expect(fs.readFileSync(yap.logPath, 'utf8')).toMatch(/Claude plan: usage asked.*how="?probe"?.*outcome="?numbers/);
  await expect(ring(main).getByRole('button', { name: 'Plan usage: 23% used' })).toBeVisible();

  // Hover: "Plan usage limits", a row per limit (the inactive scoped one at 0 % and the
  // unknown kind left out), when each resets, extra usage in euros. No chat yet: no context line.
  await ring(main).hover();
  const p = popover(main);
  await expect(p).toBeVisible();
  await expect(p.getByRole('heading', { name: 'Plan usage limits' })).toBeVisible();
  await expect(p.getByTestId('plan-usage-label')).toHaveText(['Current session', 'Weekly · all models', 'Weekly · Fable']);
  await expect(p.getByTestId('plan-usage-percent')).toHaveText(['23%', '9%', '4%']);
  const want = await expected(main);
  await expect(p.getByTestId('plan-usage-reset')).toHaveText([want.session, want.week, want.week]);
  await expect(p.getByTestId('plan-usage-extra')).toHaveText(want.extra);
  await expect(p.getByRole('meter', { name: 'Weekly · all models' })).toHaveAttribute('aria-valuenow', '9');
  await expect(p.getByTestId('plan-usage-context')).toHaveCount(0);
  await shot(main, '01-plan-usage-normal-light');

  // Pointer away: it closes. Click: it stays, focused; Esc closes it.
  await away(main);
  await expect(p).toBeHidden();
  await ring(main).getByRole('button').click();
  await expect(p).toBeFocused();
  await main.keyboard.press('Escape');
  await expect(p).toBeHidden();
  await expect(ring(main).getByRole('button')).toBeFocused();

  // See all → the plan's usage page (Manage usage's).
  await away(main);
  await ring(main).hover();
  await p.getByRole('button', { name: 'See all →' }).click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain('https://claude.ai/settings/usage');
  await expect(p).toBeHidden();
});

test('after plan turns: how full the chat is, a rate-limit event live, then the answer after the turn', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(ring(main)).toBeVisible({ timeout: 20_000 });
  await send(main, 'How is the week looking?');
  await expect(lastAnswer(main)).toContainText('First answer on the plan.', { timeout: 15_000 });
  // How full the chat is: its last call's 32,000 + 40 of 200K, never the turn's sum.
  await ring(main).hover();
  await expect(popover(main).getByTestId('plan-usage-context')).toHaveText('This chat · 16% of context');
  // Yap's own context ring isn't shown on the plan (it would read the turn's sum).
  await expect(main.getByTestId('context-ring')).toHaveCount(0);
  await expect(ring(main)).toHaveAttribute('data-percent', '23');
  await away(main);

  // The second turn: its rate-limit event moves the week to 86 % (a warning) while it's
  // still answering; once it's done, Claude Code's own answer (88 %).
  await send(main, 'And now?');
  await expect(ring(main)).toHaveAttribute('data-percent', '86', { timeout: 15_000 });
  await expect(ring(main)).toHaveAttribute('data-severity', 'warning');
  await expect(main.getByRole('button', { name: 'Stop the answer' })).toBeVisible();
  await expect(lastAnswer(main)).toContainText('Second answer on the plan.', { timeout: 15_000 });
  await expect(ring(main)).toHaveAttribute('data-percent', '88', { timeout: 10_000 });
  await expect(ring(main)).toHaveAttribute('data-severity', 'warning');
  await ring(main).hover();
  const p = popover(main);
  await expect(p.getByTestId('plan-usage-percent')).toHaveText(['23%', '88%', '4%']);
  await expect(p.getByTestId('plan-usage-row').nth(1)).toHaveAttribute('data-severity', 'warning');
  await expect(p.getByTestId('plan-usage-context')).toHaveText('This chat · 17% of context');
  expect(fs.readFileSync(yap.logPath, 'utf8')).toMatch(/Claude plan: usage asked.*how="?session"?.*outcome="?numbers/);
  await shot(main, '02-plan-usage-warning-light');
  await away(main);

  // Dark.
  await pickTheme(main, yap, 'Dark');
  await ring(main).hover();
  await expect(popover(main).getByTestId('plan-usage-context')).toBeVisible();
  await shot(main, '03-plan-usage-warning-dark');
  await away(main);
  await pickTheme(main, yap, 'System');
});

test("Settings → Chat → Claude: the plan card's usage rows under Connected", async ({ yap, main, shot }) => {
  const dialog = await claudeTab(main);
  const card = dialog.getByTestId('claude-plan-card');
  await expect(card.locator('[data-state="ready"]')).toContainText('Connected · Using Claude plan');
  const rows = card.getByTestId('claude-plan-usage');
  await expect(rows.getByTestId('plan-usage-label')).toHaveText(['Current session', 'Weekly · all models', 'Weekly · Fable'], { timeout: 15_000 });
  // The week: 88 % after the turns above (9 % on a Yap of its own).
  await expect(rows.getByTestId('plan-usage-percent')).toHaveText(['23%', /^(88|9)%$/, '4%']);
  const want = await expected(main);
  await expect(rows.getByTestId('plan-usage-reset').first()).toHaveText(want.session);
  await expect(rows.getByTestId('plan-usage-extra')).toHaveText(want.extra);
  await rows.scrollIntoViewIfNeeded();
  await shot(main, '04-settings-plan-usage-light');
  await closeSettings(main);
  await pickTheme(main, yap, 'Dark');
  const dark = await claudeTab(main);
  await expect(dark.getByTestId('claude-plan-usage').getByTestId('plan-usage-row')).toHaveCount(3);
  await dark.getByTestId('claude-plan-usage').scrollIntoViewIfNeeded();
  await shot(main, '05-settings-plan-usage-dark');
  await closeSettings(main);
  await pickTheme(main, yap, 'System');
});

test('on the API key: no plan ring, and Yap’s own context ring', async ({ yap, main }) => {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(ring(main)).toBeVisible({ timeout: 20_000 });
  // A chat answered on the plan (a session of its own: the script from the top).
  await send(main, 'Anything new?');
  await expect(lastAnswer(main)).toContainText('First answer on the plan.', { timeout: 15_000 });
  await expect(main.getByTestId('context-ring')).toHaveCount(0);
  await yap.invoke('chat_set_model', { provider: 'anthropic', model: MODEL, baseUrl: null, file: null });
  await expect(chip(main)).toContainText('Anthropic API key');
  await expect(ring(main)).toHaveCount(0);
  await expect(main.getByTestId('context-ring')).toBeVisible();
  // Back on the plan: the ring again, Yap's context ring gone.
  await yap.invoke('chat_set_model', { provider: 'claude-plan', model: MODEL, baseUrl: null, file: null });
  await expect(chip(main)).toContainText('Using Claude plan');
  await expect(ring(main)).toBeVisible();
  await expect(main.getByTestId('context-ring')).toHaveCount(0);
});
