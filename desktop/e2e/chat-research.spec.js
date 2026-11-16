// Research (desktop/src-tauri/src/chat/research.rs, chat/turn/research.rs, 16 Nov 2026; the Claude
// app's Research mode as Anthropic's help pages describe it):
// - The Research switch under the message box, beside Style (ResearchChip.svelte): on, a question is
//   researched; greyed, with why, when web search is off.
// - The plan first: a call with no tools writes it, and it's the outline card's research plan in the
//   thread ("Research plan", Start research / Edit); nothing is searched until the person answers.
// - The loop on Yap's free web sources (no search key: the stand-in's Wikipedia): searches and page
//   reads, the progress on the activity line ("… · 1 search, 0 pages read"); a tool that isn't the
//   web's refused (an app, memory), the report's words never in the thread.
// - The report: a document beside the chat, its citations only to the pages read (one only found in
//   the results goes), numbered 1, 2 as first cited, each a link to its page, a numbered Sources list;
//   in the thread the summary with its pills, the document's card and the research's chip.
// - ■ Stop mid-research: "Research stopped…", no document. A plain "research …" without the switch
//   researches too; a greeting with it on is answered as ever (the plan step says it's not research).
// The model is a local fake (support/fake-llm.js) scripted call by call; the web a local fake
// (support/fake-web.js). Nothing goes on the internet. Light and dark.
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, QWERTY_URL, SHOLES_URL, DVORAK_URL } from './support/fake-web.js';
import { endingOf } from '../src/lib/views/chat/outlineEdit.js';
import { stepsSummary } from '../src/lib/views/chat/chatSteps.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // The fake says its context, as a real server does (the web's whole toolset doesn't fit the 8K
      // Yap assumes for a server on this PC that doesn't say).
      fake.setServerModels([{ id: 'fake-chat' }]);
      fake.serveOllama({ loaded: { 'fake-chat': 32768 } });
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, fakeWeb }, use) => {
      await use({
        name: 'chat-research',
        env: fakeWeb.env,
        config: {
          // No search key: Yap's free sources (web/sources.rs), the stand-in's Wikipedia.
          webSearch: true,
          openApps: true,
          memoryLearnChats: false,
          meetingReminder: 'never',
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const lastAnswer = (main) => thread(main).locator('.cbubble.assistant').last();
const chip = (main) => main.getByTestId('chat-research-chip');
const card = (main) => thread(main).getByTestId('chat-outline-card').last();
const panel = (main) => main.getByTestId('chat-doc');
const line = (main) => main.getByTestId('chat-activity');

async function freshChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
}
async function ask(main, text) {
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}
async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}
function gate() {
  let open;
  const promise = new Promise((r) => (open = r));
  return { promise, open };
}
/** Switch Research on (or off). */
async function research(main, on) {
  if ((await chip(main).getAttribute('aria-pressed')) !== String(on)) await chip(main).click();
  await expect(chip(main)).toHaveAttribute('aria-pressed', String(on));
}

/** The number Yap gave a page this turn, from the tool results the model read. */
function numberOf(record, url) {
  for (const m of record.messages) {
    if (m.role !== 'tool') continue;
    try {
      const r = JSON.parse(m.content);
      if (r?.data?.url === url && r.data.n) return r.data.n;
      const hit = (r?.data?.results ?? []).find((x) => x.url === url);
      if (hit) return hit.n;
    } catch {
      /* not JSON */
    }
  }
  return 99;
}

/** The fake model's report: the summary and the parts, citing the two pages read and one only found. */
function reportOf(record) {
  const [q, s, d] = [numberOf(record, QWERTY_URL), numberOf(record, SHOLES_URL), numberOf(record, DVORAK_URL)];
  return [
    '# How QWERTY came about',
    '',
    '## Summary',
    '',
    `QWERTY was laid out for the Sholes and Glidden typewriter [${s}] and sold to Remington in 1873 [${q}].`,
    '',
    '## Origins',
    '',
    `Sholes patented the arrangement in 1878 [${s}, ${q}].`,
    '',
    '## Why it stayed',
    '',
    `It came with the first typewriter that sold well [${s}].`,
    '',
    '## Rivals',
    '',
    `The Dvorak layout came in 1936 [${d}].`,
    '',
    '## Sources',
    '',
    '1. Wikipedia',
  ].join('\n');
}

test.beforeEach(async ({ yap, main, fakeLlm }) => {
  fakeLlm.reply('researchPlan', null);
  await setConfig(yap, main, { theme: 'light', webSearch: true });
});

// ---------------------------------------------------------------- no app

test("the research plan's card and chips say research, not writing", () => {
  expect(endingOf('allowed', { research: true })).toBe('Researched as planned');
  expect(endingOf('changed', { research: true })).toBe('Researched with your changes');
  expect(endingOf('refused', { research: true })).toBe('Not researched');
  expect(endingOf('allowed', {})).toBe('Written as outlined');
  expect(stepsSummary(['Searched the web: "qwerty"', 'Read en.wikipedia.org/wiki/QWERTY', 'Researched for under a minute: 1 search, 1 page read', 'Wrote the report “QWERTY”'])).toBe(
    'Searched the web, read a page, researched it, wrote the report'
  );
  expect(stepsSummary(['Didn\'t search for "x": research limit: 20 searches', 'Stopped researching: research limit: 20 searches'])).toBe(
    "Stopped at the research limit, 1 didn't go through"
  );
});

// ---------------------------------------------------------------- in Chat

test('Research: the plan first, then searches and page reads on the free sources, then a report beside the chat citing only the pages read', async ({ yap, main, fakeLlm, fakeWeb, shot }) => {
  const midway = gate();
  fakeLlm.scriptChat([
    { tools: [{ name: 'web_search', args: { query: 'QWERTY keyboard layout history' } }] },
    { tools: [{ name: 'read_page', args: { url: QWERTY_URL } }, { name: 'web_search', args: { query: 'Sholes and Glidden typewriter keyboard' } }], gate: midway.promise },
    // An app and memory, asked for in a research turn: refused (the web came in too).
    { tools: [{ name: 'read_page', args: { url: SHOLES_URL } }, { name: 'open_app', args: { name: 'Calculator' } }, { name: 'remember', args: { text: 'likes typewriters' } }] },
    { content: (r) => reportOf(r) },
  ]);
  await freshChat(main);
  await research(main, true);
  await shot(main, '01-research-chip-on-light');
  const before = fakeLlm.requests.length;
  const searchesBefore = fakeWeb.requests.length;
  await ask(main, 'How did the QWERTY keyboard layout come about?');

  // The plan, before anything's looked up.
  await expect(card(main)).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card(main)).toHaveAttribute('data-research', 'true');
  await expect(card(main).locator('.otitle')).toContainText('Research plan · How did the QWERTY keyboard layout come about');
  await expect(card(main).locator('.okey')).toHaveText(['Origins', 'Why it stayed', 'Rivals']);
  await expect(card(main).getByRole('button', { name: 'Start research' })).toBeVisible();
  await expect(line(main)).toContainText('Waiting for you: the research plan');
  const planCall = fakeLlm.requests.slice(before).find((r) => r.kind === 'researchPlan');
  expect(planCall.tools).toEqual([]);
  expect(planCall.user).toContain("The user's message: How did the QWERTY keyboard layout come about?");
  expect(fakeLlm.requests.slice(before).filter((r) => r.tools.includes('web_search'))).toHaveLength(0);
  expect(fakeWeb.requests.slice(searchesBefore).filter((r) => r.host?.endsWith('wikipedia.org'))).toHaveLength(0);
  await shot(main, '02-research-plan-light');
  await theme(yap, main, 'dark');
  await shot(main, '03-research-plan-dark');
  await theme(yap, main, 'light');

  // Start: the loop, its progress on the line.
  await card(main).getByRole('button', { name: 'Start research' }).click();
  await expect.poll(() => fakeLlm.requests.slice(before).filter((r) => r.gated).length, { timeout: 20_000 }).toBe(1);
  await expect(line(main)).toContainText('1 search, 0 pages read');
  await shot(main, '04-researching-light');
  const loop = fakeLlm.requests.slice(before).find((r) => r.tools.includes('web_search'));
  expect(loop.user).toContain('This is a research turn');
  expect(loop.user).toContain('1. Origins — where it came from');
  expect(loop.user).toContain('up to 20 searches (web_search)');
  midway.open();

  // The report: beside the chat, its citations only to the pages read.
  await expect(lastAnswer(main)).toContainText('The full report is open on the right.', { timeout: 30_000 });
  await expect(card(main)).toHaveAttribute('data-state', 'allowed');
  await expect(card(main)).toContainText('Researched as planned');
  await expect(panel(main)).toBeVisible();
  const body = panel(main).locator('.docbody');
  await expect(body.locator('h1, h2').first()).toContainText('How QWERTY came about');
  await expect(body).toContainText('Researched');
  await expect(body).toContainText('2 searches, 2 pages read');
  await expect(body).toContainText('The Dvorak layout came in 1936.');
  const links = await body.locator('a').evaluateAll((as) => as.map((a) => [a.textContent.trim(), a.getAttribute('href')]));
  // Numbered as first cited: Sholes 1, QWERTY 2; the Dvorak page (only found) never.
  expect(links).toContainEqual(['[1]', SHOLES_URL]);
  expect(links).toContainEqual(['[2]', QWERTY_URL]);
  expect(links.some(([, href]) => href === DVORAK_URL)).toBe(false);
  await expect(body.locator('li').filter({ hasText: 'Sholes and Glidden typewriter' })).toHaveCount(1);
  await expect(body.locator('li').filter({ hasText: 'QWERTY - Wikipedia' }).or(body.locator('li').filter({ hasText: /^QWERTY/ }))).not.toHaveCount(0);
  // The thread: the summary and its two pills, the document's card, the research's chip.
  await expect(lastAnswer(main)).toContainText('QWERTY was laid out for the Sholes and Glidden typewriter');
  await expect(lastAnswer(main).locator('a.md-cite')).toHaveCount(2);
  await expect(thread(main).getByTestId('chat-doc-pill').last()).toContainText('How QWERTY came about');
  const sent = fakeLlm.requests.slice(before);
  // An app and memory in a research turn: refused, the model told why.
  const after = sent.find((r) => r.messages.some((m) => m.role === 'tool' && String(m.content).includes("open_app doesn't run in a research turn")));
  expect(after).toBeTruthy();
  expect(after.messages.some((m) => m.role === 'tool' && String(m.content).includes("remember doesn't run in a research turn"))).toBe(true);
  // The report's words never streamed into the thread.
  await expect(thread(main)).not.toContainText('## Origins');
  // Kept: the document is a note in Chat documents, its body as the panel shows it.
  const note = (yap.readJson('notes.json')?.notes ?? []).find((n) => n.title === 'How QWERTY came about');
  expect(note.folder).toBe('Chat documents');
  expect(note.content).toContain(`[[1]](${SHOLES_URL})`);
  expect(note.content).toContain('## Sources');
  expect(note.content).not.toContain(DVORAK_URL);
  await shot(main, '05-research-report-light');
  await theme(yap, main, 'dark');
  await shot(main, '06-research-report-dark');
  await theme(yap, main, 'light');
});

test('■ Stop mid-research: no report, and a plain "research …" researches without the switch', async ({ main, fakeLlm }) => {
  const held = gate();
  fakeLlm.scriptChat([{ tools: [{ name: 'web_search', args: { query: 'typewriter history' } }] }, { tools: [{ name: 'read_page', args: { url: QWERTY_URL } }], gate: held.promise }]);
  await freshChat(main);
  await research(main, false);
  await ask(main, 'research the history of the typewriter keyboard');
  await expect(card(main)).toHaveAttribute('data-research', 'true', { timeout: 20_000 });
  await card(main).getByRole('button', { name: 'Start research' }).click();
  await expect.poll(() => fakeLlm.requests.filter((r) => r.gated).length, { timeout: 20_000 }).toBeGreaterThan(0);
  await main.getByRole('button', { name: 'Stop the answer' }).click();
  await expect(lastAnswer(main)).toContainText('Research stopped before the report was written.', { timeout: 20_000 });
  await expect(panel(main)).toHaveCount(0);
  held.open();
});

test('a greeting with Research on is answered as ever; with web search off the switch says why', async ({ yap, main, fakeLlm }) => {
  fakeLlm.reply('researchPlan', () => '{"research": false}');
  fakeLlm.scriptChat([{ content: 'Hello! What shall we look into?' }]);
  await freshChat(main);
  await research(main, true);
  await ask(main, 'Hi there');
  await expect(lastAnswer(main)).toContainText('Hello! What shall we look into?', { timeout: 20_000 });
  await expect(thread(main).getByTestId('chat-outline-card')).toHaveCount(0);
  // Web search off: the switch can't, and says why.
  await setConfig(yap, main, { webSearch: false });
  await chip(main).click();
  await expect(chip(main)).toHaveAttribute('aria-pressed', 'false');
  await expect(chip(main)).toHaveAttribute('title', /turn on web search/);
  await setConfig(yap, main, { webSearch: true });
});
