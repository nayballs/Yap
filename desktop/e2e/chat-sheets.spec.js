// Spreadsheets (15 Nov 2026: desktop/src-tauri/src/chat/sheets.rs, chat/attachments/sheet.rs,
// desktop/src/lib/views/chat/SheetTab.svelte, sheetView.js):
// - **Read**: an .xlsx (or .xlsm, .xls, .xlsb, .ods, .csv, .tsv) attached in Chat or added to a project is read
//   on this PC: the model gets each sheet as a table — column letters across, row numbers down — with its used
//   range and its formulas, a script's formulas (no results in the file) calculated by Yap.
// - **Shown**: dropped on Chat, it opens beside the chat as Excel draws it: sheet tabs, letters and numbers,
//   widths, fonts, fills, borders, number and date formats, merged cells, frozen panes, calculated values; a
//   cell's formula in the formula bar. Its chip opens it again; Open in Excel opens the person's own app.
// - **A Code chat's**: a path in Claude's answer and its Outputs row open the workbook; changed on disk, the view
//   follows it, keeping its sheet and the cell selected. A big sheet scrolls, its rows asked for as they show.
//
// Fictional workbooks made here (support/xlsx.js). Plain Chat on a local fake model; the Code chat on Yap's
// stand-in for Claude Code (code-chat-files.spec.js' way).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat, plainSide, moreClearOf } from './support/code-chats.js';
import { seedOrderXlsx, bigXlsx } from './support/xlsx.js';
import { isSheetName, columnName, cellName, gridOf, indexAt, inView, blocksFor, spillOf, alignOf, styleCss, bordersCss, factsOf, mergeAt, shownNumber, spillNeeded } from '../src/lib/views/chat/sheetView.js';

const HERE = path.join(RUNS, 'chat-sheets-spec');
const ORDER = path.join(HERE, 'files', 'seed-order.xlsx');
const BEDS = path.join(HERE, 'files', 'beds.csv');
const SCRIPTS = path.join(HERE, 'scripts');
const ALLOTMENT = path.join(HERE, 'folders', 'allotment');
const WS_ORDER = path.join(ALLOTMENT, 'budget', 'seed-order.xlsx');
const WS_BIG = path.join(ALLOTMENT, 'beds.xlsx');
const ANSWER = 'The order is in `budget/seed-order.xlsx`: kale is the biggest line. Every bed is in beds.xlsx.';

const init = () => ({ type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: '{{session_id}}', tools: ['Bash', 'Edit', 'Read', 'Write'], mcp_servers: [{ name: 'yap', status: 'connected' }], model: 'claude-haiku-5-5', permissionMode: 'auto', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] });
const said = (id, content, stop = 'tool_use') => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content, stop_reason: stop }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const toolResult = (id, text) => ({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const result = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 900, num_turns: 2, result: text, session_id: '{{session_id}}', total_cost_usd: 0.02, usage: { input_tokens: 4000, output_tokens: 120, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
/** Claude "writes" the seed order (the file is the test's), then answers with the workbooks' paths. */
const BUILDS = [
  { _wait_user: true },
  init(),
  said('w1', [{ type: 'tool_use', id: 'toolu_write', name: 'Write', input: { file_path: '{{cwd}}\\budget\\seed-order.xlsx', content: '(a workbook)' } }]),
  { _hook: { event: 'PreToolUse', input: { tool_name: 'Write', tool_use_id: 'toolu_write', tool_input: { file_path: 'budget\\seed-order.xlsx', content: '(a workbook)' } } } },
  toolResult('toolu_write', 'File created successfully at: budget/seed-order.xlsx'),
  said('w2', [{ type: 'text', text: ANSWER }], 'end_turn'),
  result(ANSWER),
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

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
      fs.mkdirSync(path.join(HERE, 'files'), { recursive: true });
      fs.writeFileSync(ORDER, seedOrderXlsx());
      fs.writeFileSync(BEDS, 'Bed,Crop,Sown\r\n1,Broad beans,"3 March, under fleece"\r\n2,Leeks,12 April\r\n');
      await use({ name: 'chat-sheets', env: fakeClaude(writeScript('idle', [{ _wait_user: true }])), config: { ...PLAN, webSearch: false, meetingReminder: 'never' } });
    },
    { scope: 'worker' },
  ],
});

/** Chat on the person's Claude plan (Yap's stand-in for Claude Code): a Code chat's. */
const PLAN = planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false });
/** Chat on the local fake model: a plain chat's, whose requests the test reads. */
const fakeChat = (fakeLlm) => ({ chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } });

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const panel = (main) => main.getByTestId('chat-doc');
const view = (main) => main.getByTestId('chat-sheet');
const at = (main, a1) => view(main).locator(`[data-cell="${a1}"]`).first();
const drop = (yap, paths) => yap.invoke('e2e_drag_drop', { paths });
const lastAsked = (fakeLlm) => {
  const msgs = fakeLlm.requests.filter((r) => r.kind === 'other').at(-1)?.messages || [];
  const m = msgs.filter((x) => x.role === 'user').at(-1);
  return typeof m?.content === 'string' ? m.content : (m?.content || []).filter((p) => p.type === 'text').map((p) => p.text).join('\n');
};

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

async function closePanel(main) {
  if (await panel(main).count()) await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
}

// ---- the view's sums (no app)

test("the view's sums: names, the grid, what's in view, a cell's look and its text running on", () => {
  expect(['a.XLSX', 'b.xlsm', 'c.xls', 'd.xlsb', 'e.ods', 'f.csv', 'g.tsv'].every(isSheetName)).toBe(true);
  expect(isSheetName('notes.txt') || isSheetName('xlsx') || isSheetName('C:\\x\\.csv')).toBe(false);
  expect([columnName(1), columnName(26), columnName(27), columnName(16384), cellName(7, 2)]).toEqual(['A', 'Z', 'AA', 'XFD', 'B7']);
  // A sheet's grid: its widths and heights (a hidden column 0), frozen rows, a margin past what's used.
  const sheet = { drawn: { top: 1, left: 1, bottom: 10, right: 4 }, cols: [{ from: 1, to: 1, px: 140 }, { from: 3, to: 3, hidden: true }], rowLooks: [{ r: 1, px: 28 }], merges: [{ row: 1, col: 1, rows: 1, cols: 4 }], frozenRows: 2, frozenCols: 0 };
  const g = gridOf(sheet);
  expect([g.rows, g.cols, g.frozenRows]).toEqual([60, 14, 2]);
  expect([g.colAt[2], g.colAt[3], g.colAt[4], g.colAt[5]]).toEqual([140, 204, 204, 268]);
  expect([g.rowAt[2], g.rowAt[3]]).toEqual([28, 48]);
  expect([indexAt(g.colAt, 0), indexAt(g.colAt, 139), indexAt(g.colAt, 140), indexAt(g.colAt, 205), indexAt(g.rowAt, 1e9)]).toEqual([1, 1, 2, 4, 60]);
  const v = inView(g, { top: 0, left: 0, w: 400, h: 200 }, 0);
  expect(v.r0).toBe(3);
  expect(v.c0).toBe(1);
  expect(blocksFor(150, 450)).toEqual([0, 1, 2]);
  expect(mergeAt(sheet.merges, 1, 3)).toEqual(sheet.merges[0]);
  // Text runs on into empty columns, up to the next one with something in it; numbers never.
  const row = [{ c: 1, t: 'A long note about the beds', k: 'text' }, { c: 4, t: '7', k: 'number' }];
  expect(spillOf(row[0], {}, row)).toBe(2);
  expect(spillOf(row[0], { wrap: true }, row)).toBe(0);
  expect(spillOf(row[1], {}, row)).toBe(0);
  expect(spillOf(row[0], {}, row, (c) => c === 3)).toBe(1);
  expect(spillNeeded(1, 5, 150, [0, 64, 64, 64, 64, 64, 64])).toBe(2);
  // A number too wide for its cell: Excel's ####, never cut short.
  expect(shownNumber('15/11/2026', 60, 64, 8)).toBe('15/11/2026');
  expect(shownNumber('15/11/2026', 70, 64, 8)).toBe('#######');
  expect([alignOf({ k: 'number' }, {}), alignOf({ k: 'text' }, {}), alignOf({ k: 'bool' }, {}), alignOf({ k: 'number' }, { h: 'left' })]).toEqual(['right', 'left', 'center', 'left']);
  // Excel's "Automatic" text: the ink with no fill, black on one; the file's own colours kept.
  expect(styleCss({}, { font: 'Calibri' })).toBe('color:var(--sheet-ink)');
  expect(styleCss({ fill: '#DDEBF7', bold: true }, { font: 'Calibri' })).toBe('font-weight:700;background:#DDEBF7;color:#000000');
  expect(styleCss({ color: '#FF0000', size: 140, font: 'Aptos' }, { font: 'Calibri' })).toContain('font-size:18.6');
  expect(bordersCss({ bottom: { style: 'thin' } })).toBe('left:0px;top:0px;border-bottom:1px solid var(--sheet-line)');
  expect(factsOf({ calculated: true, format: 'xlsx', sheets: [{}, { hidden: true }] }, { used: { top: 1, left: 1, bottom: 6, right: 6 }, formulas: 7 })).toBe('A1:F6 · 7 formulas, calculated by Yap · 1 hidden sheet');
});

// ---- in the app

test('a workbook dropped on Chat opens beside it as Excel draws it, and the model gets its sheets as tables', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: fakeChat(fakeLlm) });
  await openView(main, 'Chat');
  await plainSide(main);
  await main.getByRole('button', { name: 'New chat' }).click();
  await drop(yap, [ORDER, BEDS]);
  const chips = main.getByTestId('chat-files').getByTestId('chat-file-chip');
  await expect(chips).toHaveCount(2);
  await expect(chips.nth(0)).toContainText('XLSX');
  await expect(chips.nth(0)).toHaveAttribute('data-kind', 'sheet');
  await expect(chips.nth(1)).toContainText('CSV');

  // The workbook beside the chat: its tabs (the hidden one not shown), its values calculated in its own formats.
  await expect(view(main)).toHaveAttribute('data-name', 'seed-order.xlsx');
  await expect(view(main).getByTestId('chat-sheet-tabs').getByRole('tab')).toHaveText(['Budget', 'Notes']);
  await expect(at(main, 'D6')).toHaveText('£23.00');
  await expect(at(main, 'D3')).toHaveText('£7.50');
  await expect(at(main, 'F3')).toHaveText('33%');
  await expect(at(main, 'E3')).toHaveText('15/11/2026');
  // The title: merged over A1:D1 (its width the four columns'), bold, on its blue fill, in the frozen rows on top.
  const title = view(main).locator('.layer.top [data-cell="A1"]');
  await expect(title).toHaveText('Seed order');
  await expect(title).toHaveCSS('width', `${140 + 56 + 64 + 64}px`);
  await expect(title).toHaveCSS('background-color', 'rgb(221, 235, 247)');
  await expect(title).toHaveCSS('font-weight', '700');
  await expect(view(main).locator('.layer.top [data-cell="A2"]')).toHaveText('Item');
  // A long note runs on into the empty columns after it as far as it needs, as Excel's do.
  const run = parseFloat(await at(main, 'A8').locator('.st').evaluate((el) => getComputedStyle(el).width));
  expect(run).toBeGreaterThan(140 + 56 + 64 + 64);
  expect(run).toBeLessThan(140 + 56 + 64 * 12);
  // The formula bar: the cell clicked, its formula.
  await at(main, 'D6').click();
  await expect(view(main).getByTestId('chat-sheet-cell')).toHaveText('D6');
  await expect(view(main).getByTestId('chat-sheet-formula')).toHaveValue('=SUM(D3:D5)');
  await view(main).getByTestId('chat-sheet-grid').press('ArrowUp');
  await expect(view(main).getByTestId('chat-sheet-formula')).toHaveValue('=B5*C5');
  await expect(view(main).getByTestId('chat-sheet-facts')).toHaveText('A1:F8 · 7 formulas, calculated by Yap · 1 hidden sheet');
  // Nothing in it is editable.
  await expect(view(main).locator('[contenteditable], textarea:not([readonly])')).toHaveCount(0);
  await expect(view(main).getByTestId('chat-sheet-formula')).toHaveAttribute('readonly', '');
  await shot(main, '01-a-workbook-beside-the-chat');
  await theme(yap, main, 'dark');
  // Dark: the paper dark, Excel's automatic text light; the file's own fill kept, black on it.
  const ink = await view(main).getByTestId('chat-sheet-grid').evaluate((el) => getComputedStyle(el).color);
  expect(ink).not.toBe('rgb(0, 0, 0)');
  await expect(at(main, 'A3')).toHaveCSS('color', ink);
  await expect(title).toHaveCSS('background-color', 'rgb(221, 235, 247)');
  await expect(title).toHaveCSS('color', 'rgb(0, 0, 0)');
  await shot(main, '02-a-workbook-beside-the-chat-dark');
  await theme(yap, main, 'light');

  // Another sheet: its italic red note.
  await view(main).getByTestId('chat-sheet-tabs').getByRole('tab', { name: 'Notes' }).click();
  await expect(at(main, 'A1')).toHaveText('Order by Friday');
  await expect(at(main, 'A1')).toHaveCSS('color', 'rgb(255, 0, 0)');
  await expect(at(main, 'A1')).toHaveCSS('font-style', 'italic');
  // Open in Excel: the person's own app (a test run keeps it).
  await view(main).getByTestId('chat-sheet-open-app').click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContainEqual('open-sheet:seed-order.xlsx');

  // Sent: each sheet as a table with its formulas, before the question; the CSV as one table.
  fakeLlm.scriptChat([{ content: 'Kale is the biggest line, at £12.00.' }]);
  await box(main).fill('Which line costs the most?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(thread(main).locator('.cbubble.assistant').last()).toHaveText('Kale is the biggest line, at £12.00.');
  const asked = lastAsked(fakeLlm);
  expect(asked).toContain("<source>seed-order.xlsx (an Excel workbook: its cells as tables, read on the person's PC)</source>");
  expect(asked).toContain('## Sheet 1 of 3: "Budget"\nUsed range A1:F8 (8 rows × 6 columns); frozen: rows 1–2; merged: A1:D1.');
  expect(asked).toContain('| 5 | Kale | 4 | £3.00 | £12.00 |  | 52% |');
  expect(asked).toContain('D3:D5: =B3*C3 (the same in each row, filled down)');
  expect(asked).toContain('<source>beds.csv (a CSV file: its cells as tables');
  expect(asked).toContain('| 2 | 1 | Broad beans | 3 March, under fleece |');
  expect(asked.trimEnd().endsWith('Which line costs the most?')).toBe(true);

  // The question's chip opens the workbook again.
  await closePanel(main);
  await expect(view(main)).toHaveCount(0);
  await thread(main).getByTestId('chat-files-sent').getByRole('button', { name: /seed-order\.xlsx/ }).click();
  await expect(at(main, 'D6')).toHaveText('£23.00');
  await closePanel(main);

  // A project's file: a workbook added is read as an attachment is.
  const p = await yap.invoke('project_create', { name: 'Allotment 2027', description: null });
  const out = await yap.invoke('project_files_add', { id: p.id, paths: [ORDER] });
  expect(out.added.map((f) => [f.name, f.kind])).toEqual([['seed-order.xlsx', 'sheet']]);
  await yap.invoke('project_delete', { id: p.id });
});

test("a Code chat's workbook opens from Claude's answer and its Outputs, follows the file as it changes, and a big one scrolls", async ({ yap, main, shot }) => {
  fs.rmSync(path.join(HERE, 'folders'), { recursive: true, force: true });
  fs.mkdirSync(path.dirname(WS_ORDER), { recursive: true });
  fs.writeFileSync(WS_ORDER, seedOrderXlsx());
  fs.writeFileSync(WS_BIG, bigXlsx(5000));
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { llmScopes: PLAN.llmScopes, agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', webSearch: false, speakReplies: false, assistantScreen: false });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'allotment')) await yap.invoke('workspaces_add', { path: ALLOTMENT });
  await yap.invoke('e2e_claude_script', { script: writeScript('builds', BUILDS) });
  const agents = () => yap.invoke('agent_list');
  const before = (await agents()).length;
  await openView(main, 'Chat');
  await closePanel(main);
  await newCodeChat(main, 'allotment');
  await box(main).fill('Make me a seed order workbook.');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(async () => (await agents()).length, { timeout: 20_000 }).toBe(before + 1);
  const agent = (await agents())[0];
  await expect.poll(async () => (await agents()).find((j) => j.id === agent.id)?.status, { timeout: 30_000 }).toBe('done');
  const answer = thread(main).locator('.cbubble.assistant').last();
  await expect(answer.locator('.md-filelink')).toHaveText(['budget/seed-order.xlsx', 'beds.xlsx']);
  // Outside the workspace: never shown.
  await expect(yap.invoke('sheet_open', { src: { from: 'workspace', conversation: agent.conversation, rel: '../../files/seed-order.xlsx' } })).rejects.toThrow(/isn't in this chat's workspace/);

  // Its path in the answer: the workbook beside the chat, live.
  await answer.locator('.md-filelink').first().click();
  await expect(view(main)).toHaveAttribute('data-name', 'seed-order.xlsx');
  await expect(view(main).getByTestId('chat-sheet-live')).toHaveText('Live');
  await expect(at(main, 'D6')).toHaveText('£23.00');
  await at(main, 'D5').click();
  await expect(view(main).getByTestId('chat-sheet-formula')).toHaveValue('=B5*C5');

  // Claude changes it on disk: the view follows, on the same sheet with the same cell selected.
  fs.writeFileSync(WS_ORDER, seedOrderXlsx({ kale: 10 }));
  await expect(at(main, 'D6')).toHaveText('£41.00', { timeout: 10_000 });
  await expect(at(main, 'D5')).toHaveText('£30.00');
  await expect(view(main).getByTestId('chat-sheet-cell')).toHaveText('D5');
  await expect(view(main)).toHaveAttribute('data-sheet', 'Budget');
  await shot(main, '03-a-code-chats-workbook-live');
  // Caught half-written: what's shown stays; whole again, it shows.
  fs.writeFileSync(WS_ORDER, Buffer.from('PK\x03\x04 half a workbook'));
  await expect(view(main).getByTestId('chat-sheet-live')).toHaveText('Changing…', { timeout: 10_000 });
  await expect(at(main, 'D6')).toHaveText('£41.00');
  fs.writeFileSync(WS_ORDER, seedOrderXlsx({ kale: 2 }));
  await expect(at(main, 'D6')).toHaveText('£17.00', { timeout: 10_000 });
  await closePanel(main);

  // Its Outputs row (the file its work changed): the workbook again.
  await main.getByTestId('chat-summary-button').click();
  const row = main.getByTestId('chat-summary').getByTestId('chat-summary-outputs').getByTestId('chat-output');
  await expect(row.first()).toContainText('seed-order.xlsx');
  await row.first().click();
  await expect(view(main)).toHaveAttribute('data-name', 'seed-order.xlsx');
  await expect(at(main, 'D6')).toHaveText('£17.00');
  await closePanel(main);

  // A big sheet: its rows come as they scroll into view, the header frozen at the top. Its link is short and
  // ends its line: its ⋯ comes up clear of it, its end still the link's (ui/fileLinks.js `moreSpot`).
  const beds = answer.locator('.md-filelink', { hasText: 'beds.xlsx' });
  await moreClearOf(beds);
  await beds.click();
  await expect(view(main)).toHaveAttribute('data-name', 'beds.xlsx');
  await expect(view(main).getByTestId('chat-sheet-facts')).toHaveText('A1:E5000 · 4,999 formulas, calculated by Yap');
  await expect(at(main, 'E2')).toHaveText('36');
  const grid = view(main).getByTestId('chat-sheet-grid');
  // Ctrl+End: the last cell used, scrolled to (the blank rows after it as Excel's grid goes on).
  await at(main, 'B3').click();
  await grid.press('Control+End');
  await expect(view(main).getByTestId('chat-sheet-cell')).toHaveText('E5000');
  await expect(view(main).getByTestId('chat-sheet-formula')).toHaveValue('=D5000*12');
  await expect(at(main, 'A5000')).toHaveText('4999');
  await expect(at(main, 'E5000')).toHaveText(String(((5000 % 7) + 1) * 12));
  await expect(view(main).locator('.layer.top [data-cell="C1"]')).toHaveText('Crop');
  // Only what's in view is drawn.
  expect(await view(main).locator('.layer.body .sc').count()).toBeLessThan(600);
  // Row 2,500 at the top, under the frozen header (rows of 20 px).
  await grid.evaluate((el) => (el.scrollTop = 2499 * 20 - 20));
  await expect(at(main, 'A2500')).toHaveText('2499');
  await expect(at(main, 'A2500')).toBeInViewport();
  await shot(main, '04-a-big-sheet-scrolled');
  await theme(yap, main, 'dark');
  await shot(main, '05-a-big-sheet-dark');
  await theme(yap, main, 'light');
  await closePanel(main);
});
