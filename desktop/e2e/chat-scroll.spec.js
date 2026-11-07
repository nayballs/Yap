// Chat's thread follows its newest line, as Claude's app, ChatGPT and Slack
// do (desktop/src/lib/core/stickToBottom.js, views/chat/ChatThread.svelte,
// 2026-11-07: "I had to drag it down when it reported back to me").
// - At the bottom it stays there while it grows: a long answer streaming
//   (checked from a ResizeObserver made after the thread's own, so it sees
//   each change once the thread has followed it), a picture that loads after
//   the answer, a job's ask-first card (its own chunk, drawn after the
//   question), its tab opening beside the chat and its report read in.
// - Scrolled up (the wheel) it stays where it was put while the answer goes
//   on, and "↓ New messages" shows; the button goes back down and follows
//   again. Light and dark, with and without "New messages".
// - A job's report while you read further up: nothing moves, "New messages".
// - The person's own question and a conversation opened: at the bottom.
//
// The model is the local fake (support/fake-llm.js); a job's Claude Code is
// this build's own stand-in (`yap.exe fake-claude`, as jobs-ui.spec.js has
// it), in this run's own folder. Fictional details only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';
import { plainSide } from './support/code-chats.js';

const HERE = path.join(RUNS, 'chat-scroll-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const GARDEN = path.join(HERE, 'folders', 'garden-planner');

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
        name: 'chat-scroll',
        config: {
          llmScopes: {
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
          },
          memoryLearnChats: false,
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

// ---- the thread

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const streamingBubble = (main) => main.getByTestId('chat-streaming');
const toBottom = (main) => main.getByTestId('chat-to-bottom');

/** How far the thread is from its bottom (CSS px). */
const gap = (main) => thread(main).evaluate((el) => Math.round(el.scrollHeight - el.scrollTop - el.clientHeight));
const scrollTop = (main) => thread(main).evaluate((el) => Math.round(el.scrollTop));
const scrolls = (main) => thread(main).evaluate((el) => el.scrollHeight > el.clientHeight + 40);

async function atBottom(main, message = 'the thread at its bottom') {
  await expect.poll(() => gap(main), { message }).toBeLessThanOrEqual(1);
}

async function freshChat(main) {
  await openView(main, 'Chat');
  // A job's tab open beside the chat from an earlier test: closed.
  const job = main.getByTestId('chat-job');
  if (await job.count()) await job.getByRole('button', { name: 'Close the panel' }).click();
  // Jobs on: a plain chat, on the Chat side (a chat that started a job is a Code chat).
  await plainSide(main);
  await main.getByRole('button', { name: 'New chat' }).click();
}

async function send(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The wheel turned up over the thread (a real wheel event, as a mouse's). */
async function wheelUp(main, dy = 400) {
  const b = await thread(main).boundingBox();
  await main.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await main.mouse.wheel(0, -dy);
  // Settled (a wheel's scroll may glide).
  let last = -1;
  await expect
    .poll(async () => {
      const now = await scrollTop(main);
      const still = now === last;
      last = now;
      return still;
    }, { intervals: [120], message: 'the wheel scroll to settle' })
    .toBe(true);
}

/**
 * Every distance from the bottom the thread showed after it changed size
 * (from a ResizeObserver made after the thread's own: it runs once the
 * thread has followed the change), while it scrolls.
 */
async function watchGaps(main) {
  await main.evaluate(() => {
    window.__gapRo?.disconnect();
    const el = document.querySelector('.msgs');
    window.__gaps = [];
    window.__gapRo = new ResizeObserver(() => {
      if (el.scrollHeight > el.clientHeight + 1) window.__gaps.push(Math.round(el.scrollHeight - el.scrollTop - el.clientHeight));
    });
    window.__gapRo.observe(el.querySelector('.msgsin'));
  });
  return () => main.evaluate(() => window.__gaps);
}

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

/** A long answer: `n` list items, each its own (an answer saying one thing over and over is a loop Yap stops). */
function longAnswer(n, lead = 'Here is the plan for the garden open day:') {
  const things = ['the gazebo', 'the seed swap table', 'the compost talk', 'the raffle', 'the tea urn', 'the plant labels', 'the bunting', 'the car park'];
  const people = ['Sam', 'Priya', 'Alex', 'Jo', 'Morgan', 'Robin'];
  const lines = [lead, ''];
  for (let i = 1; i <= n; i++) lines.push(`- Step ${i}: ${people[i % people.length]} checks ${things[i % things.length]} by ${(i % 9) + 1} o'clock on day ${(i * 7) % 28 + 1}.`);
  return lines.join('\n');
}

/** `text` streamed a line at a time. */
const byLine = (text) => text.split(/(?<=\n)/);

// ---------------------------------------------------------------- following

test('a long answer streams while you are at the bottom: the thread follows it to its last line', async ({ main, fakeLlm }) => {
  await freshChat(main);
  // (Long enough to scroll well before its end, whatever the window's height.)
  const answer = longAnswer(90);
  fakeLlm.scriptChat([{ content: answer, stream: { chunks: byLine(answer), gapMs: 40 } }]);
  await send(main, 'What is the plan for the open day?');
  await expect(thread(main).locator('.cbubble.user')).toHaveCount(1);
  const gaps = await watchGaps(main);

  await expect(streamingBubble(main)).toBeVisible();
  await expect(streamingBubble(main)).toHaveCount(0, { timeout: 20_000 });
  await expect(lastAnswer(main)).toContainText('Step 90:');
  expect(await scrolls(main)).toBe(true);
  await atBottom(main);
  // Every step of the way, after the thread followed it: at the bottom.
  const seen = await gaps();
  expect(seen.length).toBeGreaterThan(10);
  expect(Math.max(...seen)).toBeLessThanOrEqual(1);
  await expect(toBottom(main)).toHaveCount(0);
});

test('scrolled up mid-answer: it stays where you put it, "↓ New messages" shows, and goes back down', async ({ yap, main, fakeLlm, shot }) => {
  await freshChat(main);
  const answer = longAnswer(120, 'Here is the plan for the allotment fair:');
  fakeLlm.scriptChat([{ content: answer, stream: { chunks: byLine(answer), gapMs: 70 } }]);
  await send(main, 'What is the plan for the allotment fair?');
  // Mid-answer, with room to scroll up into.
  await expect(streamingBubble(main)).toBeVisible();
  await expect
    .poll(() => thread(main).evaluate((el) => el.scrollHeight - el.clientHeight), { timeout: 15_000, message: 'room to scroll up' })
    .toBeGreaterThan(450);
  await expect(streamingBubble(main)).not.toContainText('Step 100:');

  // The wheel up: let go at once, and it holds while the answer goes on.
  await wheelUp(main, 300);
  const held = await scrollTop(main);
  expect(await gap(main)).toBeGreaterThan(100);
  await expect(toBottom(main)).toBeVisible();
  const said = (await streamingBubble(main).textContent()).match(/Step (\d+)/g).length;
  await expect(thread(main)).toContainText(`Step ${Math.min(said + 4, 120)}:`);
  await expect(toBottom(main)).toContainText('New messages');
  await expect(toBottom(main)).toHaveAccessibleName('New messages: scroll to the latest');
  expect(Math.abs((await scrollTop(main)) - held)).toBeLessThanOrEqual(1);
  await shot(main, '01-new-messages-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-new-messages-dark');
  await theme(yap, main, 'light');

  // Done (the checked answer in its place): still where it was put.
  await expect(streamingBubble(main)).toHaveCount(0, { timeout: 20_000 });
  await expect(lastAnswer(main)).toContainText('Step 120:');
  expect(Math.abs((await scrollTop(main)) - held)).toBeLessThanOrEqual(1);
  await expect(toBottom(main)).toBeVisible();

  // ↓: back to the newest, following again; the box has the focus.
  await toBottom(main).click();
  await atBottom(main);
  await expect(toBottom(main)).toHaveCount(0);
  await expect(box(main)).toBeFocused();

  // Up again with nothing new since: the plain round "↓".
  await wheelUp(main, 300);
  await expect(toBottom(main)).toBeVisible();
  await expect(toBottom(main)).not.toContainText('New messages');
  await expect(toBottom(main)).toHaveAccessibleName('Scroll to the latest');
  await toBottom(main).hover();
  await shot(main, '03-to-bottom-light');
  await theme(yap, main, 'dark');
  await shot(main, '04-to-bottom-dark');
  await theme(yap, main, 'light');

  // Scrolling back down to the bottom follows again by itself.
  const b = await thread(main).boundingBox();
  await main.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await main.mouse.wheel(0, 2000);
  await atBottom(main);
  await expect(toBottom(main)).toHaveCount(0);
});

test('a picture that loads after the answer: still at the bottom', async ({ main, fakeLlm }) => {
  await freshChat(main);
  const answer = longAnswer(60, 'Here is the plan for the bulb planting:');
  fakeLlm.scriptChat([{ content: answer }]);
  await send(main, 'What is the plan for the bulb planting?');
  await expect(lastAnswer(main)).toContainText('Step 60:');
  await atBottom(main);

  // A picture in the last answer whose size comes only when it loads (as
  // an answer's picture or a page's does), a moment after the answer.
  await lastAnswer(main).evaluate((bubble) => {
    const img = document.createElement('img');
    img.alt = '';
    img.dataset.testLate = '1';
    img.style.display = 'block';
    img.style.maxWidth = '100%';
    bubble.querySelector('.cbody').append(img);
    const svg = "<svg xmlns='http://www.w3.org/2000/svg' width='480' height='320'><rect width='480' height='320' fill='%23c9a46b'/></svg>";
    setTimeout(() => (img.src = `data:image/svg+xml,${svg}`), 400);
  });
  const late = thread(main).locator('img[data-test-late]');
  await expect.poll(() => late.evaluate((img) => img.getBoundingClientRect().height)).toBeGreaterThan(100);
  await atBottom(main, 'the thread at its bottom after the picture loaded');
  await expect(toBottom(main)).toHaveCount(0);
});

test('your own question goes to the bottom, from wherever you were; a conversation opens at its bottom', async ({ main, fakeLlm }) => {
  await freshChat(main);
  const first = longAnswer(60, 'Here is the plan for the pond clean-up:');
  fakeLlm.scriptChat([{ content: first }]);
  await send(main, 'What is the plan for the pond clean-up?');
  await expect(lastAnswer(main)).toContainText('Step 60:');
  await atBottom(main);
  const id = Number(/^chat:(\d+):/.exec(await lastAnswer(main).getAttribute('data-msg-id'))[1]);

  // Read further up, then ask: the question and its answer at the bottom.
  await wheelUp(main, 500);
  await expect(toBottom(main)).toBeVisible();
  fakeLlm.scriptChat([{ content: 'Bring waders and a net.' }]);
  await send(main, 'What should I bring?');
  await expect(lastAnswer(main)).toHaveText('Bring waders and a net.');
  await atBottom(main);
  await expect(toBottom(main)).toHaveCount(0);

  // Another conversation, then this one again: it opens at its bottom.
  await main.getByRole('button', { name: 'New chat' }).click();
  await expect(thread(main)).toHaveCount(0);
  await main.locator('.convs').getByText('What is the plan for the pond clean-up?').first().click();
  await expect(lastAnswer(main)).toHaveAttribute('data-msg-id', `chat:${id}:3`);
  await atBottom(main);
  await expect(toBottom(main)).toHaveCount(0);
});

// ---------------------------------------------------------------- a job's report

const init = () => ({
  type: 'system',
  subtype: 'init',
  cwd: '{{cwd}}',
  session_id: '{{session_id}}',
  tools: ['Bash', 'Edit', 'Read', 'Write'],
  mcp_servers: [{ name: 'yap', status: 'connected' }],
  model: 'claude-sonnet-5-5',
  permissionMode: 'auto',
  apiKeySource: 'none',
  claude_code_version: '2.1.296',
  capabilities: ['interrupt_receipt_v1'],
});
const result = (text) => ({
  type: 'result',
  subtype: 'success',
  is_error: false,
  duration_ms: 1200,
  num_turns: 1,
  result: text,
  session_id: '{{session_id}}',
  total_cost_usd: 0.002,
  usage: { input_tokens: 600, output_tokens: 40, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 },
  permission_denials: [],
  terminal_reason: 'completed',
});
/** Claude Code's stand-in: a moment's work (`ms`), then done. */
const work = (ms) => [{ _wait_user: true }, init(), { _sleep_ms: ms }, result('Done: the beds are planned.'), { _wait_user: true }];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

const jobs = (yap) => yap.invoke('job_list');

/** Jobs on, `garden-planner` fresh, Claude Code's stand-in working `ms` before it's done. */
async function jobsReady(yap, main, ms) {
  fs.rmSync(GARDEN, { recursive: true, force: true });
  fs.mkdirSync(GARDEN, { recursive: true });
  fs.writeFileSync(path.join(GARDEN, 'notes.md'), 'Beds to plan\n');
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { jobsEnabled: true, jobsAsk: 'new-or-web', jobsSpeak: 'never', jobsPay: 'plan', jobsMode: 'auto', speakReplies: false });
  await yap.invoke('e2e_claude_script', { script: writeScript(`work-${ms}`, work(ms)) });
}

/** A conversation long enough to scroll: a question and a long answer, at the bottom. */
async function longChat(main, fakeLlm, question, lead) {
  await freshChat(main);
  fakeLlm.scriptChat([{ content: longAnswer(60, lead) }]);
  await send(main, question);
  await expect(lastAnswer(main)).toContainText('Step 60:');
  expect(await scrolls(main)).toBe(true);
  await atBottom(main);
}

/** The model's turns for a job in `folder`: it starts it, says so, and later reports `report`. */
function jobTurns(fakeLlm, folder, report) {
  fakeLlm.scriptChat([
    { tools: [{ name: 'start_job', args: { folder, brief: 'Plan the raised beds in notes.md.' } }] },
    { content: "On it: I've started a job in garden-planner. I'll tell you when it's done." },
    { plain: true, content: report },
  ]);
}

/** Every part of `els` inside the thread's view. */
async function inView(main, els) {
  const view = await thread(main).boundingBox();
  for (const el of els) {
    const b = await el.boundingBox();
    expect(b.y).toBeGreaterThanOrEqual(view.y - 1);
    expect(b.y + b.height).toBeLessThanOrEqual(view.y + view.height + 1);
  }
}

test("a job while you're at the bottom: its ask-first card, its tab opening beside the chat, its report — each in view", async ({ yap, main, fakeLlm, shot }) => {
  const REPORT = 'Done. I planned the raised beds in notes.md: leeks, kale and broad beans, with the paths between them.';
  await jobsReady(yap, main, 1500);
  // A folder jobs haven't worked in: Yap asks first, with a card in the thread.
  await yap.invoke('job_folders_remove', { name: 'garden-planner' }).catch(() => {});
  await longChat(main, fakeLlm, 'What could go in the raised beds?', 'Here are some ideas for the raised beds:');
  const before = (await jobs(yap)).length;
  jobTurns(fakeLlm, GARDEN, REPORT);
  await send(main, `Start a job in ${GARDEN} to plan the raised beds.`);

  const card = thread(main).getByTestId('chat-job-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking');
  await atBottom(main, 'the thread at its bottom with the ask-first card');
  await inView(main, [card]);

  // Started: its tab opens beside the chat (the thread narrower, its lines longer).
  await card.getByRole('button', { name: 'Start' }).click();
  await expect.poll(async () => (await jobs(yap)).length, { message: 'a job to start' }).toBe(before + 1);
  await expect(main.getByTestId('chat-job')).toBeVisible();
  await expect(lastAnswer(main)).toContainText("I'll tell you when it's done.");
  await atBottom(main, 'the thread at its bottom with the job tab open');

  // The report: the job's card, then the chat model's own words.
  await expect(thread(main).getByTestId('chat-job-report')).toBeVisible({ timeout: 30_000 });
  await expect(lastAnswer(main)).toHaveText(REPORT);
  await atBottom(main, 'the thread at its bottom after the report');
  await expect(toBottom(main)).toHaveCount(0);
  await inView(main, [thread(main).getByTestId('chat-job-report'), lastAnswer(main)]);
  await shot(main, '05-job-report-followed');
});

test("a job's report while you read further up: nothing moves, and “New messages” says it came", async ({ yap, main, fakeLlm }) => {
  const REPORT = 'Done. The raised beds are planned in notes.md, three of them along the fence.';
  await jobsReady(yap, main, 5000);
  if (!(await yap.invoke('job_folders_list')).some((f) => f.name === 'garden-planner')) await yap.invoke('job_folders_add', { path: GARDEN });
  await longChat(main, fakeLlm, 'What grows well along a fence?', 'Here is what grows well along a fence:');
  const before = (await jobs(yap)).length;
  jobTurns(fakeLlm, 'garden-planner', REPORT);
  await send(main, 'Start a job in garden-planner to plan the beds by the fence.');
  await expect.poll(async () => (await jobs(yap)).length, { message: 'a job to start' }).toBe(before + 1);
  const job = (await jobs(yap))[0];
  await expect(lastAnswer(main)).toContainText("I'll tell you when it's done.");
  await atBottom(main);

  await wheelUp(main, 400);
  const held = await scrollTop(main);
  await expect(toBottom(main)).toBeVisible();
  await expect(toBottom(main)).not.toContainText('New messages');
  await expect.poll(async () => (await jobs(yap)).find((j) => j.id === job.id)?.status, { timeout: 30_000 }).toBe('done');

  await expect(thread(main).getByTestId('chat-job-report')).toHaveCount(1, { timeout: 30_000 });
  await expect(lastAnswer(main)).toHaveText(REPORT);
  await expect(toBottom(main)).toContainText('New messages');
  expect(Math.abs((await scrollTop(main)) - held)).toBeLessThanOrEqual(1);

  await toBottom(main).click();
  await atBottom(main);
  await inView(main, [thread(main).getByTestId('chat-job-report'), lastAnswer(main)]);
});
