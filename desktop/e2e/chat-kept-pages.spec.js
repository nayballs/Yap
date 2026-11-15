// Pages Yap keeps (`save_page`: desktop/src-tauri/src/chat/made/pages.rs, made/serve.rs's `/p/…`;
// desktop/src/lib/views/chat/KeptPageTab.svelte, PageCard.svelte, OutputsTab.svelte, outputs.js; after the
// Claude app's artifacts — local only, never published):
// - "Make me a tip calculator I can keep": the page saved in the chat's folder (chat-made/<chat>/:
//   made.json, `<id>.v1.html`), opened beside the chat at once in the same sandboxed frame as a visual (an
//   opaque origin), a card in its answer ("Tip calculator · Saved ›").
// - A change asked for: save_page with its id makes version 2 — a card of its own ("Version 2"), the panel
//   on the newest, ‹ › between versions; the first card opens version 1.
// - The chat's Outputs (the panel's button, the conversation's ⋯ → Outputs; the one list, outputs.js, as a
//   Code chat's summary has it): the page ("Page · 2 versions"), the documents it wrote and the files it
//   shared, each opening where it lives.
// - Kept: shown again when the chat opens again.
// - Refused: an id that isn't this chat's page.
// The model is the scripted fake AI (support/fake-llm.js), named like a strong model. Fictional content
// only; nothing leaves the PC. Light and dark.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, setConfig, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { RUNS } from './support/yap.js';

const NOW = Math.floor(Date.now() / 1000);
const page = (pct) => `<!doctype html><html><head><title>Tips</title><style>
body{font-family:var(--font)}.card{max-width:340px;margin:24px auto;padding:18px;border:1px solid var(--border);border-radius:12px;background:var(--surface)}
label{display:block;margin:10px 0 4px;color:var(--muted)}output{font-size:22px;color:var(--text)}</style></head>
<body><div class="card"><h1 id="name">Tip calculator</h1><label>Bill</label><input id="bill" type="number" value="40">
<label>Tip <span id="pct">${pct}</span>%</label><input id="tip" type="range" min="0" max="30" value="${pct}">
<p>Tip: <output id="out"></output></p></div>
<script>const f=()=>{const b=+bill.value||0,t=+tip.value;pct.textContent=t;out.textContent='£'+(b*t/100).toFixed(2)};bill.oninput=tip.oninput=f;f();</script>
</body></html>`;

const HERE = path.join(RUNS, 'chat-kept-pages-spec');
const NOTES_FILE = path.join(HERE, 'packing.md');

// A chat with a document it wrote and a file it shared (seeded), and a page it kept (seeded).
const SEEDED_PAGE = 'pg_00000000005eed01';
const CHATS = {
  conversations: [
    {
      id: 81,
      title: 'Weekend trip',
      createdTs: NOW - 900,
      updatedTs: NOW - 880,
      messages: [
        { role: 'user', text: 'Plan the weekend and keep a packing list page', ts: NOW - 900 },
        {
          role: 'assistant',
          text: 'The plan is in the document, the list is a page.',
          ts: NOW - 899,
          documents: [{ id: 9_101, title: 'Weekend plan' }],
          files: [{ caption: 'Packing notes', items: [{ name: 'packing.md', path: NOTES_FILE, folder: HERE, kind: 'markdown', bytes: 40 }] }],
        },
      ],
    },
  ],
};

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
      fs.rmSync(HERE, { recursive: true, force: true });
      fs.mkdirSync(HERE, { recursive: true });
      fs.writeFileSync(NOTES_FILE, '# Packing\n\n- Boots\n- Map\n');
      await use({
        name: 'chat-kept-pages',
        data: { 'chats.json': CHATS },
        config: {
          webSearch: false,
          meetingReminder: 'never',
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5' } },
        },
        prepare: (dir) => {
          const folder = path.join(dir, 'chat-made', '81');
          fs.mkdirSync(folder, { recursive: true });
          const after = { index: 0, ts: NOW - 900 };
          const item = { id: SEEDED_PAGE, kind: 'page', title: 'Packing list', format: 'html', after, at: (NOW - 899) * 1000, bytes: 30, versions: [{ n: 1, at: (NOW - 899) * 1000, bytes: 30, after, title: 'Packing list' }] };
          fs.writeFileSync(path.join(folder, 'made.json'), JSON.stringify({ version: 1, created: NOW - 900, items: [item] }));
          fs.writeFileSync(path.join(folder, `${SEEDED_PAGE}.v1.html`), '<ul><li>Boots</li><li>Map</li></ul>');
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const cards = (main) => main.getByTestId('chat-page-card');
const kept = (main) => main.getByTestId('chat-kept-page');

async function ask(main, text, { fresh = true } = {}) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  if (fresh) await main.getByRole('button', { name: 'New chat' }).click();
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function openConversation(main, title) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  await main.locator('.conv').filter({ hasText: title }).click();
}

/** The kept page's frame (a Playwright Frame), once loaded. */
async function keptFrame(main) {
  const el = kept(main).getByTestId('chat-kept-frame');
  await expect(el).toBeVisible({ timeout: 20_000 });
  let frame = null;
  await expect
    .poll(async () => {
      frame = await (await el.elementHandle()).contentFrame();
      return frame ? await frame.evaluate(() => document.readyState).catch(() => '') : '';
    })
    .toBe('complete');
  return frame;
}

const toolResult = (fake, from, nth = 1) => JSON.parse(fake.requests.slice(from).filter((r) => r.tools?.length)[nth].messages.filter((m) => m.role === 'tool').at(-1).content);

test('a page to keep: saved, beside the chat, a card a version, ‹ › between them, kept', async ({ yap, main, fakeLlm, shot }) => {
  let from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'save_page', args: { title: 'Tip calculator', html: page(15) } }] },
    { content: 'It’s open beside the chat and kept in this chat.' },
  ]);
  await ask(main, 'Make me a tip calculator I can keep');
  await expect(main.getByText('It’s open beside the chat and kept')).toBeVisible({ timeout: 20_000 });
  // Offered, and kept.
  expect(fakeLlm.requests.slice(from).find((r) => r.tools?.length).tools).toEqual(expect.arrayContaining(['show_visual', 'save_page']));
  const first = toolResult(fakeLlm, from);
  expect(first).toMatchObject({ success: true, displayText: 'Saved a page', data: { version: 1, saved: 'Saved the page “Tip calculator”' } });
  const id = first.data.id;
  expect(id).toMatch(/^pg_[0-9a-f]{16}$/);

  // Beside the chat at once, in the sandboxed frame; its card in the answer.
  await expect(kept(main)).toBeVisible();
  await expect(kept(main).getByRole('heading', { name: 'Tip calculator' })).toBeVisible();
  const el = kept(main).getByTestId('chat-kept-frame');
  await expect(el).toHaveAttribute('sandbox', 'allow-scripts');
  expect(await el.getAttribute('src')).toMatch(new RegExp(`^http://yapmade\\.localhost/p%2F\\d+%2F${id}%2F1$`));
  let frame = await keptFrame(main);
  expect(await frame.evaluate(() => String(self.origin))).toBe('null');
  await expect(frame.locator('#pct')).toHaveText('15');
  await expect(frame.locator('#out')).toHaveText('£6.00');
  // It works: the slider moves the tip.
  await frame.locator('#tip').fill('20');
  await expect(frame.locator('#out')).toHaveText('£8.00');
  await expect(cards(main)).toHaveCount(1);
  await expect(cards(main)).toContainText('Tip calculator');
  await expect(cards(main)).toContainText('Saved');
  await expect(main.getByTestId('chat-steps').last()).toContainText('Saved a page');
  await shot(main, '01-page-saved');

  // A change: its next version, a card of its own, the panel on the newest.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'save_page', args: { id, title: 'Tip calculator', html: page(20) } }] },
    { content: 'Now 20% is the default.' },
  ]);
  await ask(main, 'Make 20% the default', { fresh: false });
  await expect(main.getByText('Now 20% is the default.')).toBeVisible({ timeout: 20_000 });
  expect(toolResult(fakeLlm, from)).toMatchObject({ success: true, displayText: 'Saved version 2 of a page', data: { id, version: 2 } });
  await expect(cards(main)).toHaveCount(2);
  await expect(cards(main).nth(1)).toContainText('Version 2');
  await expect(kept(main).getByTestId('chat-kept-version')).toHaveText('Version 2 of 2');
  frame = await keptFrame(main);
  await expect(frame.locator('#pct')).toHaveText('20');
  // ‹: version 1, as it was.
  await kept(main).getByTestId('chat-kept-earlier').click();
  await expect(kept(main).getByTestId('chat-kept-version')).toHaveText('Version 1 of 2');
  frame = await keptFrame(main);
  await expect(frame.locator('#pct')).toHaveText('15');
  await kept(main).getByTestId('chat-kept-later').click();
  await expect(kept(main).getByTestId('chat-kept-version')).toHaveText('Version 2 of 2');
  // The first card: version 1.
  await cards(main).first().click();
  await expect(kept(main).getByTestId('chat-kept-version')).toHaveText('Version 1 of 2');
  await shot(main, '02-two-versions');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  frame = await keptFrame(main);
  await expect.poll(() => frame.evaluate(() => document.documentElement.dataset.theme)).toBe('dark');
  await shot(main, '03-two-versions-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Kept in the chat's own folder.
  const conv = yap.readJson('chats.json').conversations.find((c) => c.messages.some((m) => m.text?.includes('tip calculator I can keep')));
  const folder = path.join(yap.dataDir, 'chat-made', String(conv.id));
  const index = JSON.parse(fs.readFileSync(path.join(folder, 'made.json'), 'utf8'));
  expect(index.items).toEqual([expect.objectContaining({ id, kind: 'page', title: 'Tip calculator', versions: [expect.objectContaining({ n: 1, after: { index: 0, ts: conv.messages[0].ts } }), expect.objectContaining({ n: 2, after: { index: 2, ts: conv.messages[2].ts } })] })]);
  expect(fs.readFileSync(path.join(folder, `${id}.v1.html`), 'utf8')).toBe(page(15));
  expect(fs.readFileSync(path.join(folder, `${id}.v2.html`), 'utf8')).toBe(page(20));

  // Outputs (the title bar's button): the page, two versions; it opens.
  await main.getByTestId('chat-outputs-button').click();
  const outputs = main.getByTestId('chat-outputs');
  await expect(outputs).toBeVisible();
  const row = outputs.getByTestId('chat-output').filter({ hasText: 'Tip calculator' });
  await expect(row).toContainText('Page · 2 versions');
  await row.click();
  await expect(kept(main).getByTestId('chat-kept-version')).toHaveText('Version 2 of 2');

  // Shown again when the chat opens again.
  await openConversation(main, 'Weekend trip');
  await openConversation(main, conv.title);
  await expect(cards(main)).toHaveCount(2);
});

test('Outputs: every page, document and file the chat made, from its ⋯', async ({ main, shot }) => {
  await openConversation(main, 'Weekend trip');
  // Its page's card, in the answer that saved it.
  await expect(cards(main)).toHaveCount(1);
  await expect(cards(main)).toContainText('Packing list');
  // The conversation's ⋯ → Outputs.
  await main.locator('.conv').filter({ hasText: 'Weekend trip' }).click({ button: 'right' });
  await main.getByRole('menuitem', { name: 'Outputs' }).click();
  const outputs = main.getByTestId('chat-outputs');
  await expect(outputs).toBeVisible();
  const rows = outputs.getByTestId('chat-output');
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText('Packing list');
  await expect(rows.nth(0)).toHaveAttribute('data-kind', 'page');
  await expect(rows.nth(1)).toContainText('Weekend plan');
  await expect(rows.nth(1)).toContainText('Document');
  await expect(rows.nth(2)).toContainText('packing.md');
  await expect(rows.nth(2)).toContainText('Shared');
  await shot(main, '04-outputs');
  // The page opens beside the chat.
  await rows.nth(0).click();
  await expect(kept(main).getByRole('heading', { name: 'Packing list' })).toBeVisible();
  const frame = await keptFrame(main);
  await expect(frame.locator('li').first()).toHaveText('Boots');
  // Back to Outputs from its tab; then closed.
  await main.getByTestId('chat-panel-tab').filter({ hasText: 'Outputs' }).click();
  await expect(outputs).toBeVisible();
  // The title bar's button (pressed while Outputs shows) closes the panel, and opens Outputs again from the page.
  const button = main.getByTestId('chat-outputs-button');
  await expect(button).toHaveAttribute('aria-pressed', 'true');
  await button.click();
  await expect(outputs).toHaveCount(0);
  await expect(button).toHaveAttribute('aria-pressed', 'false');
  await button.click();
  await expect(outputs).toBeVisible();
  // The panel's row has the tabs and +, and none of the chat's own buttons (they're the title bar's).
  await expect(main.getByTestId('chat-panel-outputs')).toHaveCount(0);
  await expect(main.getByTestId('chat-panel-summary')).toHaveCount(0);
});

test('refused: a page id that isn’t this chat’s', async ({ main, fakeLlm }) => {
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'save_page', args: { id: SEEDED_PAGE, title: 'Packing list', html: '<p>Changed</p>' } }] },
    { content: 'I couldn’t change that page.' },
  ]);
  await ask(main, 'Change the packing list page');
  await expect(main.getByText('I couldn’t change that page.')).toBeVisible({ timeout: 20_000 });
  expect(toolResult(fakeLlm, from)).toMatchObject({ success: false, displayText: "Didn't save a page: no such page here" });
  await expect(cards(main)).toHaveCount(0);
});
