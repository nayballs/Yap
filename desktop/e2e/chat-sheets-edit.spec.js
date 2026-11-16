// Editing a spreadsheet beside the chat (16 Nov 2026: desktop/src-tauri/src/chat/sheets/edit.rs, save.rs, patch.rs;
// desktop/src/lib/views/chat/SheetTab.svelte):
// - **Edited**: a Code chat's .xlsx or .csv — a value or a formula typed in the cell or the formula bar; Enter, Tab,
//   the arrows, Esc, F2 and Delete as in Excel; calculated on this PC (IronCalc); Ctrl+Z / Ctrl+Y and ↶ ↷.
// - **Saved back**: a moment after the change, only the cells changed written into the file (its other sheets, its
//   styles, every other part as it was); a CSV's one record, in its own line endings.
// - **Never over someone else's change**: changed on disk under changes not saved, nothing is written and a bar
//   offers Reload or Save as a copy.
// - **Read only**: an attached copy (said when you try to type); and live and read only while Claude works in the
//   chat's folder.
//
// Fictional workbooks made here (support/xlsx.js); the Code chat on Yap's stand-in for Claude Code.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { fakeClaude, planConfig } from './support/claude-plan.js';
import { RUNS } from './support/yap.js';
import { newCodeChat, plainSide } from './support/code-chats.js';
import { seedOrderXlsx, partsOf } from './support/xlsx.js';

const HERE = path.join(RUNS, 'chat-sheets-edit-spec');
const SCRIPTS = path.join(HERE, 'scripts');
const ORCHARD = path.join(HERE, 'folders', 'orchard');
const WS_ORDER = path.join(ORCHARD, 'budget', 'seed-order.xlsx');
const WS_COPY = path.join(ORCHARD, 'budget', 'seed-order (copy).xlsx');
const WS_BEDS = path.join(ORCHARD, 'beds.csv');
const ATTACHED = path.join(HERE, 'files', 'seed-order.xlsx');
const BEDS_CSV = 'Bed,Crop,Sown\r\n1,"Broad beans","3 March, under fleece"\r\n2,Leeks,12 April\r\n';
const ANSWER = 'The seed order is in `budget/seed-order.xlsx`, and the beds are in `beds.csv`.';

const init = () => ({ type: 'system', subtype: 'init', cwd: '{{cwd}}', session_id: '{{session_id}}', tools: ['Bash', 'Edit', 'Read', 'Write'], mcp_servers: [{ name: 'yap', status: 'connected' }], model: 'claude-haiku-5-5', permissionMode: 'auto', apiKeySource: 'none', claude_code_version: '2.1.296', capabilities: ['interrupt_receipt_v1'] });
const said = (id, content, stop = 'tool_use') => ({ type: 'assistant', message: { id, type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content, stop_reason: stop }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const toolResult = (id, text) => ({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content: text }] }, parent_tool_use_id: null, session_id: '{{session_id}}' });
const result = (text) => ({ type: 'result', subtype: 'success', is_error: false, duration_ms: 900, num_turns: 1, result: text, session_id: '{{session_id}}', total_cost_usd: 0.01, usage: { input_tokens: 3000, output_tokens: 60, cache_creation_input_tokens: 0, cache_read_input_tokens: 0 }, permission_denials: [], terminal_reason: 'completed' });
/** Claude answers with the files' paths. */
const SCRIPT = [{ _wait_user: true }, init(), said('a1', [{ type: 'text', text: ANSWER }], 'end_turn'), result(ANSWER), { _wait_user: true }];
/** A turn of its own (each turn is a Claude Code of its own): it reads the beds for a while, at work in the folder. */
const SLOW = [
  { _wait_user: true },
  init(),
  said('b1', [{ type: 'tool_use', id: 'toolu_read', name: 'Read', input: { file_path: '{{cwd}}\\beds.csv' } }]),
  { _sleep_ms: 9000 },
  toolResult('toolu_read', 'Bed,Crop,Sown'),
  said('b2', [{ type: 'text', text: 'The beds are as they were.' }], 'end_turn'),
  result('The beds are as they were.'),
  { _wait_user: true },
];

function writeScript(name, lines) {
  fs.mkdirSync(SCRIPTS, { recursive: true });
  const file = path.join(SCRIPTS, `${name}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
  return file;
}

const PLAN = planConfig({ 'claude-plan': 'claude-plan' }, { memoryLearnChats: false });

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
    async ({}, use) => {
      fs.rmSync(HERE, { recursive: true, force: true });
      fs.mkdirSync(path.join(HERE, 'files'), { recursive: true });
      fs.writeFileSync(ATTACHED, seedOrderXlsx());
      await use({ name: 'chat-sheets-edit', env: fakeClaude(writeScript('idle', [{ _wait_user: true }])), config: { ...PLAN, webSearch: false, meetingReminder: 'never' } });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const thread = (main) => main.locator('.msgs');
const panel = (main) => main.getByTestId('chat-doc');
const view = (main) => main.getByTestId('chat-sheet');
const grid = (main) => view(main).getByTestId('chat-sheet-grid');
const at = (main, a1) => view(main).locator(`[data-cell="${a1}"]`).first();
const editor = (main) => view(main).getByTestId('chat-sheet-editor');
const formula = (main) => view(main).getByTestId('chat-sheet-formula');
const where = (main) => view(main).getByTestId('chat-sheet-cell');
const saved = (main) => view(main).getByTestId('chat-sheet-save');
const hint = (main) => view(main).getByTestId('chat-sheet-hint');
const sheetXml = (file) => partsOf(fs.readFileSync(file))['xl/worksheets/sheet1.xml'];

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}

async function closePanel(main) {
  if (await panel(main).count()) await panel(main).getByRole('button', { name: 'Close the panel' }).first().click();
}

/** The answer with the files' links (the Code chat made once). */
let answer = null;
let agentId = null;

async function codeChat(yap, main) {
  if (answer) return answer;
  fs.rmSync(path.join(HERE, 'folders'), { recursive: true, force: true });
  fs.mkdirSync(path.dirname(WS_ORDER), { recursive: true });
  fs.writeFileSync(WS_ORDER, seedOrderXlsx());
  fs.writeFileSync(WS_BEDS, BEDS_CSV);
  await yap.invoke('e2e_claude_approve_auto', { allow: true });
  await setConfig(yap, main, { llmScopes: PLAN.llmScopes, agentsEnabled: true, agentsDirect: true, agentsAsk: 'new-or-web', agentsSpeak: 'never', agentsMode: 'auto', webSearch: false, speakReplies: false, assistantScreen: false });
  if (!(await yap.invoke('workspaces_list')).some((f) => f.name === 'orchard')) await yap.invoke('workspaces_add', { path: ORCHARD });
  await yap.invoke('e2e_claude_script', { script: writeScript('orchard', SCRIPT) });
  const agents = () => yap.invoke('agent_list');
  const before = (await agents()).length;
  await openView(main, 'Chat');
  await closePanel(main);
  await newCodeChat(main, 'orchard');
  await box(main).fill('Where are the seed order and the beds?');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(async () => (await agents()).length, { timeout: 20_000 }).toBe(before + 1);
  agentId = (await agents())[0].id;
  await expect.poll(async () => (await agents()).find((j) => j.id === agentId)?.status, { timeout: 30_000 }).toBe('done');
  // (The answer with the links, whatever comes after it.)
  answer = thread(main).locator('.cbubble.assistant', { has: main.locator('.md-filelink', { hasText: 'seed-order.xlsx' }) }).first();
  await expect(answer.locator('.md-filelink')).toHaveText(['budget/seed-order.xlsx', 'beds.csv']);
  return answer;
}

async function openOrder(yap, main) {
  // (A later test: Chat again, and the Code chat from its list.)
  if (answer) {
    await openView(main, 'Chat');
    if (!(await answer.isVisible())) await main.getByRole('button', { name: /^Where are the seed order and the beds\?/ }).first().click();
  }
  const a = await codeChat(yap, main);
  await expect(a).toBeVisible();
  await a.locator('.md-filelink', { hasText: 'seed-order.xlsx' }).click();
  await expect(view(main)).toHaveAttribute('data-name', 'seed-order.xlsx');
  await expect(grid(main)).toHaveAttribute('data-editable', 'true');
}

test("a Code chat's workbook: typed in, calculated, saved back with only its cells changed, Undo and Redo", async ({ yap, main, shot }) => {
  await openOrder(yap, main);
  const original = partsOf(fs.readFileSync(WS_ORDER));
  await expect(at(main, 'D6')).toHaveText('£23.00');

  // Typed in the cell: kale's price 3 → 4, Enter. Calculated at once, its pounds kept, the selection a row down.
  await at(main, 'C5').click();
  await expect(formula(main)).toHaveValue('3');
  await grid(main).press('4');
  await expect(editor(main)).toHaveValue('4');
  await editor(main).press('Enter');
  await expect(at(main, 'C5')).toHaveText('£4.00');
  await expect(at(main, 'D5')).toHaveText('£16.00');
  await expect(at(main, 'D6')).toHaveText('£27.00');
  await expect(where(main)).toHaveText('C6');
  // Saved to the file a moment later: the cell and the total in it, every other part as it was.
  await expect(saved(main)).toHaveText('Saved', { timeout: 10_000 });
  const parts = partsOf(fs.readFileSync(WS_ORDER));
  expect(parts['xl/worksheets/sheet1.xml']).toContain('<c r="C5" s="4"><v>4</v></c>');
  expect(parts['xl/worksheets/sheet1.xml']).toContain('<f>SUM(D3:D5)</f><v>27</v>');
  for (const name of ['xl/worksheets/sheet2.xml', 'xl/worksheets/sheet3.xml', 'xl/styles.xml', '[Content_Types].xml']) expect(parts[name], name).toBe(original[name]);
  expect(parts['xl/workbook.xml']).toContain('fullCalcOnLoad="1"');
  // Live, and our own save isn't someone else's change: no bar.
  await expect(view(main).getByTestId('chat-sheet-conflict')).toHaveCount(0);
  await shot(main, '01-a-cell-changed-and-saved');

  // The formula bar: B3's 3 → 5, Tab ends it and moves right.
  await at(main, 'B3').click();
  await formula(main).click();
  await formula(main).fill('5');
  await expect(editor(main)).toHaveValue('5');
  await formula(main).press('Tab');
  await expect(at(main, 'B3')).toHaveText('5');
  await expect(at(main, 'D3')).toHaveText('£12.50');
  await expect(where(main)).toHaveText('C3');
  // A formula typed in an empty cell (G3, four to the right); an arrow ends what's typed and moves.
  for (let i = 0; i < 4; i++) await grid(main).press('ArrowRight');
  await expect(where(main)).toHaveText('G3');
  await grid(main).press('=');
  await editor(main).pressSequentially('SUM(B3:B5)');
  await editor(main).press('ArrowDown');
  await expect(at(main, 'G3')).toHaveText('11');
  await expect(where(main)).toHaveText('G4');
  await at(main, 'G3').click();
  await expect(formula(main)).toHaveValue('=SUM(B3:B5)');
  // Esc lets what's typed go.
  await at(main, 'A3').click();
  await grid(main).press('X');
  await expect(editor(main)).toHaveValue('X');
  await editor(main).press('Escape');
  await expect(editor(main)).toHaveCount(0);
  await expect(at(main, 'A3')).toHaveText('Carrots');
  // F2: its edit mode, where an arrow moves the caret (still typing).
  await at(main, 'D3').click();
  await grid(main).press('F2');
  await expect(editor(main)).toHaveValue('=B3*C3');
  await editor(main).press('ArrowLeft');
  await expect(editor(main)).toBeVisible();
  await editor(main).press('Escape');
  await shot(main, '02-a-formula-typed');

  // Delete empties a cell; Ctrl+Z puts it back, then the formula's gone, then B3's 3 again; Ctrl+Y redoes; ↶ undoes.
  await at(main, 'G3').click();
  await grid(main).press('Delete');
  await expect(view(main).locator('[data-cell="G3"]')).toHaveCount(0);
  await grid(main).press('Control+z');
  await expect(at(main, 'G3')).toHaveText('11');
  await grid(main).press('Control+z');
  await expect(view(main).locator('[data-cell="G3"]')).toHaveCount(0);
  await grid(main).press('Control+z');
  await expect(at(main, 'B3')).toHaveText('3');
  await expect(where(main)).toHaveText('B3', { timeout: 5_000 });
  await grid(main).press('Control+y');
  await expect(at(main, 'B3')).toHaveText('5');
  await view(main).getByTestId('chat-sheet-undo').click();
  await expect(at(main, 'B3')).toHaveText('3');
  await expect(at(main, 'D3')).toHaveText('£7.50');
  await expect(saved(main)).toHaveText('Saved', { timeout: 10_000 });
  await expect.poll(() => sheetXml(WS_ORDER)).toContain('<c r="B3"><v>3</v></c>');
  expect(sheetXml(WS_ORDER)).not.toContain('r="G3"');
  await theme(yap, main, 'dark');
  await at(main, 'C5').click();
  await grid(main).press('5');
  await shot(main, '03-typing-in-a-cell-dark');
  await editor(main).press('Escape');
  await theme(yap, main, 'light');

  // A CSV: its one record changed, the others as written, its line endings kept.
  await closePanel(main);
  await answer.locator('.md-filelink', { hasText: 'beds.csv' }).click();
  await expect(view(main)).toHaveAttribute('data-name', 'beds.csv');
  await at(main, 'B3').click();
  await grid(main).press('O');
  await editor(main).pressSequentially('nions, red');
  await editor(main).press('Enter');
  await expect(at(main, 'B3')).toHaveText('Onions, red');
  await expect(saved(main)).toHaveText('Saved', { timeout: 10_000 });
  expect(fs.readFileSync(WS_BEDS, 'utf8')).toBe('Bed,Crop,Sown\r\n1,"Broad beans","3 March, under fleece"\r\n2,"Onions, red",12 April\r\n');
  await shot(main, '04-a-csv-changed');

  // While Claude works in the chat's folder: live and read only, said when you try; editable again after.
  await yap.invoke('e2e_claude_script', { script: writeScript('slow', SLOW) });
  await box(main).fill('Check the beds.');
  await main.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(view(main).getByTestId('chat-sheet-live')).toHaveText('Live · read only', { timeout: 10_000 });
  await expect(grid(main)).toHaveAttribute('data-editable', 'false');
  await at(main, 'B2').click();
  await grid(main).press('K');
  await expect(hint(main)).toContainText('Claude is working');
  await expect(editor(main)).toHaveCount(0);
  await shot(main, '05-read-only-while-claude-works');
  await expect(grid(main)).toHaveAttribute('data-editable', 'true', { timeout: 20_000 });
  await expect(view(main).getByTestId('chat-sheet-live')).toHaveText(/Live|Updated/);
  await closePanel(main);
});

test('changed on disk under changes not saved: never written over; Save as a copy, or Reload', async ({ yap, main, shot }) => {
  fs.mkdirSync(path.dirname(WS_ORDER), { recursive: true });
  fs.writeFileSync(WS_ORDER, seedOrderXlsx());
  await openOrder(yap, main);
  await expect(at(main, 'D5')).toHaveText('£12.00');
  // A change Yap hasn't saved yet (made straight through Rust, so the page's save doesn't go), then someone else's.
  const id = Number(await view(main).getAttribute('data-book'));
  await yap.invoke('sheet_edit', { id, sheet: 0, row: 5, col: 3, input: '9' });
  const theirs = seedOrderXlsx({ kale: 6 });
  fs.writeFileSync(WS_ORDER, theirs);
  const bar = view(main).getByTestId('chat-sheet-conflict');
  await expect(bar).toBeVisible({ timeout: 10_000 });
  await expect(bar).toContainText("seed-order.xlsx changed on disk since Yap read it, so your changes here aren't saved.");
  await expect(grid(main)).toHaveAttribute('data-editable', 'false');
  expect(fs.readFileSync(WS_ORDER).equals(theirs)).toBe(true);
  await shot(main, '06-changed-on-disk');
  // Save as a copy: beside it, with the change; the copy shown; the original theirs.
  await bar.getByRole('button', { name: 'Save as a copy' }).click();
  await expect(view(main)).toHaveAttribute('data-name', 'seed-order (copy).xlsx');
  await expect(at(main, 'D5')).toHaveText('£36.00');
  expect(sheetXml(WS_COPY)).toContain('<c r="C5" s="4"><v>9</v></c>');
  expect(fs.readFileSync(WS_ORDER).equals(theirs)).toBe(true);
  await closePanel(main);
  // Reload: the changes here let go, the file as it is on disk.
  await openOrder(yap, main);
  await expect(at(main, 'D5')).toHaveText('£18.00');
  const id2 = Number(await view(main).getAttribute('data-book'));
  await yap.invoke('sheet_edit', { id: id2, sheet: 0, row: 5, col: 3, input: '7' });
  fs.writeFileSync(WS_ORDER, seedOrderXlsx({ kale: 8 }));
  await expect(bar).toBeVisible({ timeout: 10_000 });
  await bar.getByRole('button', { name: 'Reload' }).click();
  await expect(bar).toHaveCount(0);
  await expect(at(main, 'D5')).toHaveText('£24.00');
  await expect(grid(main)).toHaveAttribute('data-editable', 'true');
  await closePanel(main);
});

test('an attached copy is read only, and says so when you try to type', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } } });
  await openView(main, 'Chat');
  await closePanel(main);
  await plainSide(main);
  await main.getByRole('button', { name: 'New chat' }).click();
  await yap.invoke('e2e_drag_drop', { paths: [ATTACHED] });
  await expect(view(main)).toHaveAttribute('data-name', 'seed-order.xlsx');
  await expect(view(main).getByTestId('chat-sheet-readonly')).toHaveText('Read only');
  await expect(grid(main)).toHaveAttribute('data-editable', 'false');
  await expect(formula(main)).toHaveAttribute('readonly', '');
  await at(main, 'C5').click();
  await grid(main).press('4');
  await expect(hint(main)).toHaveText("This is the copy attached to the chat, so it's read only.");
  await expect(editor(main)).toHaveCount(0);
  await expect(at(main, 'C5')).toHaveText('£3.00');
  await expect(view(main).getByTestId('chat-sheet-undo')).toHaveCount(0);
  await shot(main, '07-an-attached-copy-read-only');
  await closePanel(main);
});
